use crate::AsyncRunner;
use std::convert::Infallible;

use anyhow::anyhow;

use log::info;
use tokio::{runtime::Handle, sync::oneshot::{self, Sender}};

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
    use tokio::runtime::Handle;
use log::warn;
    use werkflow_scripting::Script;

    use crate::{work::Workload, AgentCommand, AgentController};

    use super::*;

    pub async fn print_status<'a>(
        controller: AgentController,
    ) -> Result<impl warp::Reply, Infallible> {
        Ok(format!(
            "The current status is: {:?}",
            controller.agent.read().status()
        ))
    }

    pub async fn start_job<'a>(
        controller: AgentController,
        script: Script,
    ) -> Result<impl warp::Reply, Infallible> {

        let mut agent = controller.agent.write();
        
        let wl_handle = agent.run(Workload::with_script(controller.clone(), script));        

        agent.runtime.as_ref().unwrap().spawn(async move {
            
            let id = wl_handle.read().await.id;
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
            .command(AgentCommand::Start).unwrap();

        Ok(format!("The agent has been started."))
    }
}

pub struct WebFeature {
    config: FeatureConfig,
    shutdown: Option<Sender<()>>,
    agent: Option<AgentController>,
}

impl WebFeature {
    pub fn new(config: FeatureConfig) -> FeatureHandle {
        FeatureHandle::new(WebFeature {
            config: config.clone(),
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
        
        return format!("Web Feature (running on port {})", self.config.bind_port).to_string();
    }

    fn on_event(&mut self, event: AgentEvent) {
        match event {
            AgentEvent::Started => {
                info!("Starting the web service");
                let controller = self.agent.clone().unwrap();                     
                let config = self.config.clone();
                let (tx, rx) = oneshot::channel();
                
                self.shutdown = Some(tx);

                let api = agent_status(controller.clone())
                    .or(filters::stop_agent(controller.clone()))
                    .or(filters::start_agent(controller.clone()))
                    .or(filters::start_job(controller.clone()))
                    .or(filters::list_jobs(controller.clone()));

                let server = warp::serve(api);

                info!("Spawning a webserver on {:?} {:?}", config.bind_address, config.bind_port);

                let (_, srv) = server.bind_with_graceful_shutdown(
                    (config.bind_address, config.bind_port),
                    async {
                        rx.await.ok();
                   }
                );
            
                controller.with_read(|f| {
                    f.runtime.as_ref().unwrap().spawn(srv);
                }); 
                

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
            AgentEvent::PayloadReceived(_) => {}
            AgentEvent::WorkStarted(_) => {}
            AgentEvent::WorkErrored(_) => {}
            AgentEvent::WorkComplete(_) => {}
        }
    }
}

#[cfg(test)]
mod test {
    use tokio::runtime::Builder;
use super::*;
    use crate::Runtime;

    #[test]
    fn web_test() {
        std::env::set_var("RUST_LOG", "trace");
        
        pretty_env_logger::init();
        let runtime  = Builder::new()
        .threaded_scheduler()
        .enable_all()
        .build().unwrap();

        let handle = &runtime.handle().clone();
        let _agt = AgentController::new("Agent");

        let mut agent = AgentController::with_runtime("Test", runtime);            
        handle.block_on(async move {
            agent
            .add_feature(WebFeature::new(FeatureConfig {
                bind_address: [127,0,0,1],
                bind_port: 3030,
                settings: Default::default(),
            }))
            .start().await
            .recv().unwrap();   
        })   
    }
}
