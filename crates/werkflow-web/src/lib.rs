use handlebars::Handlebars;
use std::{net::Ipv4Addr, sync::Arc, fs};
use tokio::sync::RwLock;
use werkflow_agents::cfg::ConfigDefinition;
use werkflow_agents::prom::{self, register_custom_metrics};
use werkflow_scripting::state::HostState;



use log::info;
use tokio::sync::oneshot::{self, Sender};

//use handlebars::Handlebars;

use warp::Filter;

use werkflow_agents::{comm::AgentEvent, AgentController, Feature, FeatureHandle};

use self::filters::agent_status;

use serde::{Deserialize, Serialize};

pub mod model;

mod filters;
mod handlers;

pub struct WebFeature {
    config: WebConfiguration,
    shutdown: Option<Sender<()>>,
    agent: Option<AgentController>,
}

impl<'a> WebFeature {
    pub fn new(config: impl ConfigDefinition + Serialize) -> FeatureHandle {
        FeatureHandle::new(WebFeature {
            config: WebConfiguration::merge(config).expect("To merge configs"),
            shutdown: None,
            agent: None,
        })
    }
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct WebConfiguration {
    pub bind_address: Ipv4Addr,
    pub port: u16,
    pub tls: Option<TlsConfiguration>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct TlsConfiguration {
    pub private_key_path: String,
    pub certificate_path: String,
}

impl Default for WebConfiguration {
    fn default() -> Self {
        WebConfiguration {
            bind_address: "127.0.0.1".parse().unwrap(),
            port: 3030,
            tls: Some(TlsConfiguration {
                private_key_path: "config/agent.key".into(),
                certificate_path: "config/agent.crt".into(),
            }),
        }
    }
}

impl ConfigDefinition for WebConfiguration {}

impl ConfigDefinition for TlsConfiguration {}

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
                if let Some(_) = self.shutdown {
                    return;
                }

                info!("Starting the web service");

                let state = Arc::new(RwLock::new(HostState::new()));
                let controller = self.agent.clone().unwrap();
                let config = self.config.clone();
                let (tx, rx) = oneshot::channel();

                self.shutdown = Some(tx);

                register_custom_metrics();

                // Use a log wrapper to add metrics to all of the calls.
                let log = warp::log::custom(|info| {
                    prom::INCOMING_REQUESTS.inc();
                    prom::RESPONSE_CODE_COLLECTOR
                        .with_label_values(&[
                            "production",
                            &info.status().as_str(),
                            &info.method().to_string(),
                        ])
                        .inc();
                });

                let mut hb = Handlebars::new();
                
                let paths = fs::read_dir("./templates").expect("Could not iterate templates");

                for p in paths {
                    let file = p.unwrap();
                    let file_name = file.file_name();
                    let file_name = file_name.to_str().unwrap();
                    
                    hb
                        .register_template_file(file_name, file.path())
                        .expect("load template file");
                    info!("Registered template {}", file_name)
                }

                let hb = Arc::new(hb);
                

                let api = agent_status(controller.clone())
                    .or(filters::stop_agent(controller.clone()))
                    .or(filters::start_agent(controller.clone()))
                    .or(filters::start_job(controller.clone()))
                    .or(filters::list_jobs(controller.clone()))
                    .or(filters::templates(controller.clone(), state.clone(), hb.clone()))
                    .or(filters::metrics())
                    .with(log);

                let server = if let Some(tls) = config.tls {
                    warp::serve(api)
                        .tls()
                        .key_path(tls.private_key_path)
                        .cert_path(tls.certificate_path)
                } else {
                    // Insecure stuff sucks
                    panic!("Web feature is not supported without TLS.")
                };

                info!(
                    "Spawning a webserver on {:?} {:?}",
                    config.bind_address, config.port
                );

                let (_, srv) =
                    server.bind_with_graceful_shutdown((config.bind_address, config.port), async {
                        rx.await.ok();
                    });

                controller.with_read(|f| {
                    f.runtime.spawn(srv);
                });

                info!("Webservice spawned into another thread.");
            }
            AgentEvent::Stopped => {
                /*if let Some(signal) = self.shutdown.take() {
                    info!("Stopping the web service");
                    let _ = signal
                        .send(())
                        .map_err(|_err| anyhow!("Error sending signal to web service"));
                }*/
                info!("Got stop signal, keeping webservice running so the agent can be started again.")
            }
            AgentEvent::PayloadReceived(_payload) => {
                // todo
            }
            AgentEvent::WorkStarted(_workload) => {
                // todo
            }
            AgentEvent::WorkErrored(_err) => {
                // todo
            }
            AgentEvent::WorkComplete(_result) => {
                // todo
            }
        }
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use std::time::Duration;

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
            let chan = agent
                .add_feature(WebFeature::new(WebConfiguration {
                    bind_address: "127.0.0.1".parse().unwrap(),
                    port: 3030,
                    tls: None,
                }))
                .start()
                .await;
            let signal = agent.signal.as_ref().unwrap().clone();

            handle.spawn(async move {
                let _time = tokio::time::interval(Duration::from_millis(10000));
                signal.send(()).unwrap();
            });
            chan.recv().unwrap();
        })
    }
}
