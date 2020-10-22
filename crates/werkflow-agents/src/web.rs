use std::{convert::Infallible, marker::PhantomData, sync::RwLockWriteGuard};

use std::{cell::RefCell, sync::Arc};
use tokio::sync::RwLock;
use tokio::{runtime::Runtime, task::JoinHandle};

//use handlebars::Handlebars;

use config::Config;
use warp::{Filter, Server};

use lazy_static::*;

type ArcAgent = Arc<RwLock<Agent>>;

use crate::{Agent, AgentHandle, Feature, FeatureConfig, FeatureHandle};

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
        println!("Start job started");
        let handle = agent_handle.handle.clone();
        
        println!("Getting writeable agent handle");
        let mut agent = handle.write().await;
        
        println!("Running");

        let wl_handle = agent.run(Workload::with_script(agent_handle.clone(), script));

        println!("Reading ID");
        let id = wl_handle.read().await.id;

        println!("Spawning Join Handler");

        agent.runtime.spawn(async move {              
             let mut handle = wl_handle.write().await;
               
             let h = handle.join_handle.as_mut().unwrap(); 
               
             if let Ok(result) = h.await.unwrap() {
                println!("Got result {}", result.result);
                
                handle.result = Some(result.result)
             } else {
                println!("Got errorin script");
             }
  
             println!("Writing status");
             handle.status = crate::work::WorkloadStatus::Complete;
        });

        println!("Spawned Monitor on Join Handle");
        
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
            vec.push(format!("Job: {}, status: {}",wh.id, wh.status));
        }

        Ok(serde_json::to_string(&vec).unwrap())
    }

    pub async fn stop_agent(agent: AgentHandle) -> Result<impl warp::Reply, Infallible> {
        agent.handle.write().await.command(AgentCommand::Stop).await;

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
}

impl<'a> WebFeature {
    pub fn new(config: FeatureConfig) -> FeatureHandle {
        FeatureHandle::new(WebFeature {
            config: config.clone(),
        })
    }
}
impl Feature for WebFeature {
    fn init(&self, agent: AgentHandle, runtime: &tokio::runtime::Handle) -> JoinHandle<()> {
        let api = agent_status(agent.clone())
            .or(filters::stop_agent(agent.clone()))
            .or(filters::start_agent(agent.clone()))
            .or(filters::start_job(agent.clone()))
            .or(filters::list_jobs(agent.clone()));

        let server = warp::serve(api);
        println!("Spawning Server");
        let jh = runtime.spawn(server.bind((self.config.bind_address, self.config.bind_port)));
        println!("Done spawning server");
        jh
    }

    fn name(&self) -> String {
        return format!("Web Feature (running on port {})", self.config.bind_port).to_string();
    }
}

#[cfg(test)]
mod test {
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
