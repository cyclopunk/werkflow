use std::net::Ipv4Addr;
use crate::cfg::ConfigDefinition;
use crate::prom::register_custom_metrics;

use std::convert::Infallible;

use anyhow::anyhow;

use log::info;
use tokio::{
    sync::oneshot::{self, Sender},
};

//use handlebars::Handlebars;

use warp::Filter;

use crate::{comm::AgentEvent, AgentController, Feature, FeatureConfig, FeatureHandle};

use self::filters::agent_status;

use serde::{Deserialize, Serialize};

mod filters {
    use werkflow_scripting::Script;

    use crate::AgentController;

    use super::*;
    fn with_agent(
        agent: AgentController,
    ) -> impl Filter<Extract = (AgentController,), Error = std::convert::Infallible> + Clone {
        warp::any().map(move || agent.clone())
    }
    pub fn metrics() -> impl Filter<Extract = impl warp::Reply, Error = warp::Rejection> + Clone {
        warp::path!("metrics")
            .and(warp::get())
            .and_then(handlers::metrics_handler)
    }
    pub fn agent_status(
        agent: AgentController,
    ) -> impl Filter<Extract = impl warp::Reply, Error = warp::Rejection> + Clone {
        warp::path!("status")
            .and(warp::get())
            .and(warp::any().map(move || agent.clone()))
            .and_then(handlers::print_status)
    }
    pub fn start_job(
        agent: AgentController,
    ) -> impl Filter<Extract = impl warp::Reply, Error = warp::Rejection> + Clone {
        warp::path!("run")
            .and(warp::post())
            .and(warp::any().map(move || agent.clone()))
            .and(warp::filters::body::json::<Script>())
            .and_then(handlers::start_job)
    }
    pub fn stop_agent(
        agent: AgentController,
    ) -> impl Filter<Extract = impl warp::Reply, Error = warp::Rejection> + Clone {
        warp::path!("stop")
            .and(warp::post())
            .and(with_agent(agent))
            .and_then(handlers::stop_agent)
    }
    pub fn start_agent(
        agent: AgentController,
    ) -> impl Filter<Extract = impl warp::Reply, Error = warp::Rejection> + Clone {
        warp::path!("start")
            .and(warp::post())
            .and(with_agent(agent))
            .and_then(handlers::start_agent)
    }
    pub fn list_jobs(
        agent: AgentController,
    ) -> impl Filter<Extract = impl warp::Reply, Error = warp::Rejection> + Clone {
        warp::path!("jobs")
            .and(warp::get())
            .and(with_agent(agent))
            .and_then(handlers::list_jobs)
    }
}
mod model {
    use super::*;
    #[derive(Serialize, Deserialize)]
    pub struct JobResult {
        pub id: u128,
        pub status: String,
        pub result_string: String,
    }

    #[derive(Serialize, Deserialize, Clone)]
    pub struct AgentInformation {
        pub name: String,
        pub jobs_ran: u32,
        pub address: String,
        pub connected_agents: Vec<u32>,
    }
}
mod handlers {
    use warp::Rejection;
    use warp::Reply;
    use werkflow_scripting::Script;

    use crate::{work::Workload, AgentCommand, AgentController};

    use super::*;

    pub async fn metrics_handler() -> Result<impl Reply, Rejection> {
        use prometheus::Encoder;
        let encoder = prometheus::TextEncoder::new();

        let mut buffer = Vec::new();
        if let Err(e) = encoder.encode(&crate::prom::REGISTRY.gather(), &mut buffer) {
            eprintln!("could not encode custom metrics: {}", e);
        };
        let mut res = match String::from_utf8(buffer.clone()) {
            Ok(v) => v,
            Err(e) => {
                eprintln!("custom metrics could not be from_utf8'd: {}", e);
                String::default()
            }
        };
        buffer.clear();

        let mut buffer = Vec::new();
        if let Err(e) = encoder.encode(&prometheus::gather(), &mut buffer) {
            eprintln!("could not encode prometheus metrics: {}", e);
        };
        let res_custom = match String::from_utf8(buffer.clone()) {
            Ok(v) => v,
            Err(e) => {
                eprintln!("prometheus metrics could not be from_utf8'd: {}", e);
                String::default()
            }
        };
        buffer.clear();

        res.push_str(&res_custom);
        Ok(res)
    }

    pub async fn print_status<'a>(
        controller: AgentController,
    ) -> Result<impl warp::Reply, Infallible> {
        Ok(format!(
            "{} The current status is: {:?}",
            controller.agent.read().name,
            controller.agent.read().status()
        ))
    }

    pub async fn start_job<'a>(
        controller: AgentController,
        script: Script,
    ) -> Result<impl warp::Reply, Infallible> {
        let mut agent = controller.agent.write();

        let wl_handle = agent.run(Workload::with_script(controller.clone(), script));

        agent.runtime.spawn(async move {
            let _id = wl_handle.read().await.id;
            let mut handle = wl_handle.write().await;
            let jh = wl_handle.write().await.join_handle.take();

            if let Some(h) = jh {
                match h
                    .await
                    .map_err(|err| anyhow!("Could not join job thread. {}", err))
                    .unwrap()
                {
                    Ok(result) => {
                        handle.result = Some(result);
                    }
                    Err(err) => {
                        anyhow!("Workload error thrown: {}", err);
                    }
                }

                handle.status = crate::work::WorkloadStatus::Complete;
            }
        });

        Ok(format!("Started job"))
    }

    pub async fn list_jobs<'a>(
        controller: AgentController,
    ) -> Result<impl warp::Reply, Infallible> {
        let work = controller.agent.read().work_handles.clone();

        let mut vec: Vec<model::JobResult> = Vec::new();

        for jh in work {
            let wh = jh.read().await;

            vec.push(model::JobResult {
                id: wh.id,
                status: wh.status.to_string(),
                result_string: wh.result.clone().unwrap_or_default(),
            });
        }

        Ok(serde_json::to_string(&vec).unwrap())
    }

    pub async fn stop_agent(controller: AgentController) -> Result<impl warp::Reply, Infallible> {
        let mut agent = controller.agent.write();

        let _ = agent.command(AgentCommand::Stop).unwrap();

        agent.work_handles.clear();

        Ok(format!("The agent has been stopped."))
    }
    pub async fn start_agent(controller: AgentController) -> Result<impl warp::Reply, Infallible> {
        let _ = controller
            .agent
            .write()
            .command(AgentCommand::Start)
            .unwrap();

        Ok(format!("The agent has been started."))
    }
}
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct WebConfiguration {
    pub bind_address: Ipv4Addr,
    pub port: u16,
    pub tls: Option<TlsConfiguration>
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct TlsConfiguration {
    pub private_key_path : String,
    pub certificate_path : String
}

impl Default for WebConfiguration {
    fn default() -> Self {
        WebConfiguration {
            bind_address: "127.0.0.1".parse().unwrap(),
            port: 3030,
            tls: Some(TlsConfiguration {
                private_key_path: "config/agent.key".into(),
                certificate_path: "config/agent.crt".into()
            })
        }
    }
}

impl ConfigDefinition for WebConfiguration {

}
impl ConfigDefinition for TlsConfiguration {

}

pub struct WebFeature {
    config: WebConfiguration,
    shutdown: Option<Sender<()>>,
    agent: Option<AgentController>,
}

impl <'a> WebFeature {
    pub fn new(config: impl ConfigDefinition + Serialize) -> FeatureHandle {
        FeatureHandle::new(WebFeature {
            config: WebConfiguration::merge(config).expect("To merge configs"),
            shutdown: None,
            agent: None,
        })
    }
}

impl Feature for WebFeature {
    fn init(&mut self, agent: AgentController) {
        self.agent = Some(agent);
    }

    fn name(&self) -> String {
        return format!("Web Feature (running on port {})", self.config.port).to_string();
    }

    fn on_event(&mut self, event: AgentEvent) {
        match event {
            AgentEvent::Started => {
                info!("Starting the web service");
                let controller = self.agent.clone().unwrap();
                let config = self.config.clone();
                let (tx, rx) = oneshot::channel();

                self.shutdown = Some(tx);

                // Add custom metrics 
                
                let log = warp::log::custom(|info| {
                    // Use a log macro, or slog, or println, or whatever!
                    crate::prom::INCOMING_REQUESTS.inc();
                    crate::prom::RESPONSE_CODE_COLLECTOR
                        .with_label_values(&[
                            "production",
                            &info.status().as_str(),
                            &info.method().to_string(),
                        ])
                        .inc();
                });

                let api = agent_status(controller.clone())
                    .or(filters::stop_agent(controller.clone()))
                    .or(filters::start_agent(controller.clone()))
                    .or(filters::start_job(controller.clone()))
                    .or(filters::list_jobs(controller.clone()))
                    .or(filters::metrics())
                    .with(log);

                let mut server;

                if let Some(tls) = config.tls {
                    server = warp::serve(api)
                        .tls()
                        .key_path(tls.private_key_path)
                        .cert_path(tls.certificate_path);
                } else  {
                    panic!("Web feature is not supported without TLS.")
                }

                info!(
                    "Spawning a webserver on {:?} {:?}",
                    config.bind_address, config.port
                );

                let (_, srv) = server.bind_with_graceful_shutdown(
                    (config.bind_address, config.port),
                    async {
                        rx.await.ok();
                    },
                );

                controller.with_read(|f| {
                    f.runtime.spawn(srv);
                });

                register_custom_metrics();

                info!("Webservice spawned into another thread.");
            }
            AgentEvent::Stopped => {
                if let Some(signal) = self.shutdown.take() {
                    info!("Stopping the web service");
                    let _ = signal
                        .send(())
                        .map_err(|_err| anyhow!("Error sending signal to web service"));
                }
            }
            AgentEvent::PayloadReceived(_payload) => {}
            AgentEvent::WorkStarted(_workload) => {}
            AgentEvent::WorkErrored(_err) => {}
            AgentEvent::WorkComplete(_result) => {}
        }
    }
}

#[cfg(test)]
mod test {
    use super::*;
    
    use tokio::runtime::Builder;

    #[test]
    fn web_test() {
        std::env::set_var("RUST_LOG", "trace");

        pretty_env_logger::init();
        let runtime = Builder::new()
            .threaded_scheduler()
            .enable_all()
            .build()
            .unwrap();

        let handle = &runtime.handle().clone();

        let mut agent = AgentController::with_runtime("Test", runtime);

        handle.block_on(async move {
            agent
                .add_feature(WebFeature::new(WebConfiguration {
                    bind_address: "127.0.0.1".parse().unwrap(),
                    port: 3030                    
                }))
                .start()
                .await
                .recv()
                .unwrap();
        })
    }
}
