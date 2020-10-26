
use std::{convert::Infallible};

use std::{sync::Arc};
use anyhow::anyhow;
use tokio::sync::{RwLock, oneshot::{self, Sender}};

//use handlebars::Handlebars;

use config::Config;
use warp::{Filter};

use lazy_static::*;

use crate::{Agent, AgentHandle, Feature, FeatureConfig, FeatureHandle, comm::AgentEvent};

use self::filters::agent_status;

lazy_static! {
    pub static ref SETTINGS: RwLock<Config> = RwLock::new(Config::default());
}

mod filters {
        use werkflow_scripting::Script;

use crate::AgentHandle;

    use super::*;

    fn with_agent(
        agent: AgentHandle,
    ) -> impl Filter<Extract = (AgentHandle,), Error = std::convert::Infallible> + Clone {
        warp::any().map(move || agent.clone())
    }

    pub fn agent_status(
        agent: AgentHandle,
    ) -> impl Filter<Extract = impl warp::Reply, Error = warp::Rejection> + Clone {
        warp::path!("status")
            .and(warp::get())
            .and(warp::any().map(move || agent.clone()))
            .and_then(handlers::print_status)
    }
    pub fn start_job(
        agent: AgentHandle,
    ) -> impl Filter<Extract = impl warp::Reply, Error = warp::Rejection> + Clone {
        warp::path!("run")
            .and(warp::post())
            .and(warp::any().map(move || agent.clone()))            
            .and(warp::filters::body::json::<Script>())
            .and_then(handlers::start_job)
    }
    pub fn stop_agent(
        agent: AgentHandle,
    ) -> impl Filter<Extract = impl warp::Reply, Error = warp::Rejection> + Clone {
        warp::path!("stop")
            .and(warp::get())
            .and(with_agent(agent))
            .and_then(handlers::stop_agent)
    }
    pub fn start_agent(
        agent: AgentHandle,
    ) -> impl Filter<Extract = impl warp::Reply, Error = warp::Rejection> + Clone {
        warp::path!("start")
            .and(warp::get())
            .and(with_agent(agent))
            .and_then(handlers::start_agent)
    }
    pub fn list_jobs(
        agent: AgentHandle,
    ) -> impl Filter<Extract = impl warp::Reply, Error = warp::Rejection> + Clone {
        warp::path!("jobs")
            .and(warp::get())
            .and(with_agent(agent))
            .and_then(handlers::list_jobs)
    }
}

mod handlers {
        use werkflow_scripting::Script;

use crate::{AgentCommand, AgentHandle, work::Workload};

    use super::*;

    pub async fn print_status<'a>(agent: AgentHandle) -> Result<impl warp::Reply, Infallible> {
        Ok(format!(
            "The current status is: {:?}",
            agent.handle.read().await.status().await
        ))
    }

    pub async fn start_job<'a>(agent_handle: AgentHandle, script: Script) -> Result<impl warp::Reply, Infallible> {
        let handle = agent_handle.handle.clone();
        
        let mut agent = handle.write().await;

        let wl_handle = agent.run(Workload::with_script(agent_handle.clone(), script));

        let id = wl_handle.read().await.id;

        agent.runtime.spawn(async move {              
             let mut handle = wl_handle.write().await;
               
             if let Some(h) = handle.join_handle.take() {                 
                match h.await.map_err(|err| anyhow!("Could not join job thread. {}", err)).unwrap() {
                    Ok(result) => {                    
                        handle.result = Some(result);
                    }
                    Err(err) => {
                        anyhow!("Workload error thrown: {}", err);
                    }
                } 

                handle.status = crate::work::WorkloadStatus::Complete;
             }
             drop(handle);
        });

        println!("Spawned Monitor on Join Handle");

        drop(agent);
        
        Ok(format!(
            "Started job {}", id
        ))
    }
    pub async fn list_jobs<'a>(agent: AgentHandle) -> Result<impl warp::Reply, Infallible> {
        println!("Aquiring read on Agent");
        
        let handle = agent.handle.read().await;
        println!("Got read on agent.");

        let mut vec: Vec<String> = Vec::new();

        for jh in &handle.work_handles {
            println!("Aquiring read on work handle");
            let wh = jh.read().await;
            println!("Got read on work handle");
            vec.push(format!("Job: {}, status: {} result: {}",wh.id, wh.status, wh.result.as_ref().unwrap_or(&"".to_string())));
        }

        Ok(serde_json::to_string(&vec).unwrap())
    }

    pub async fn stop_agent(agent: AgentHandle) -> Result<impl warp::Reply, Infallible> {
        let mut agent = agent.handle.write().await;

        agent.command(AgentCommand::Stop).await;

        agent.work_handles.clear();

        Ok(format!("The agent has been stopped."))
    }
    pub async fn start_agent(agent: AgentHandle) -> Result<impl warp::Reply, Infallible> {
        agent
            .handle
            .write()
            .await
            .command(AgentCommand::Start)
            .await;

        Ok(format!("The agent has been started."))
    }
}

pub struct WebFeature {
    config: FeatureConfig,
    shutdown: Option<Sender<()>>,
    agent: Option<AgentHandle>
}

impl<'a> WebFeature {
    pub fn new(config: FeatureConfig) -> FeatureHandle {
        FeatureHandle::new(WebFeature {
            config: config.clone(),
            shutdown: None,
            agent: None
        })
    }
}

impl Feature for WebFeature {
    fn init(&mut self, agent: AgentHandle) {
        
        self.agent = Some(agent.clone());
        
    }

    fn name(&self) -> String {
        return format!("Web Feature (running on port {})", self.config.bind_port).to_string();
    }
    
    fn on_event(&mut self, event: AgentEvent) { 
        match event {
            AgentEvent::Started => {
                println!("Got agent started event");
            let agent = self.agent.clone().unwrap();

            let api = agent_status(agent.clone())
                .or(filters::stop_agent(agent.clone()))
                .or(filters::start_agent(agent.clone()))
                .or(filters::start_job(agent.clone()))
                .or(filters::list_jobs(agent.clone()));

                let server = warp::serve(api);
                let (tx,rx) = oneshot::channel();
                
                self.shutdown = Some(tx);

                let (_, srv) = server.bind_with_graceful_shutdown((self.config.bind_address, self.config.bind_port), async move {   
                    println!("Waiting for Shutdown");         
                    rx.await.ok();
                    println!("Got shutdown!");
                });

                agent.with_read(|f| {
                    f.runtime.spawn(srv);
                });                
            }
            AgentEvent::Stopped => {
                if let Some(signal) = self.shutdown.take() {
                    println!("Stopping the web service");
                    let _ = signal.send(()).map_err(|_err| anyhow!("Error sending signal to web service"));
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
    use crate::Runtime;
use super::*;

    macro_rules! aw {
        ($e:expr) => {
            tokio_test::block_on($e)
        };
    }
    #[test]
    fn web_test() {
        let mut runtime = Runtime::new().unwrap();
        let mut agt = AgentHandle::new();
    }
}
