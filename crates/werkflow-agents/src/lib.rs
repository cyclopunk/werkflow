use tokio::sync::RwLockWriteGuard;
use crossbeam_channel::{Receiver, Sender};
use std::{collections::HashMap, fmt::Debug, ops::DerefMut};
use anyhow::{anyhow, Result};
use comm::{AgentEvent, Hub};
use std::{
    marker::PhantomData,
    sync::{
        Arc, Mutex,
    },
};

use log::info;
use tokio::{runtime::Runtime, sync::RwLock, task::JoinHandle, sync::RwLockReadGuard};
use werkflow_scripting::{Dynamic, Script};
use work::{Workload, WorkloadHandle};
use serde::{Serialize, Deserialize};
pub mod comm;
pub mod web;
pub mod work;
pub mod threads;

impl<'a> Default for Agent {
    fn default() -> Agent {
        let agt = Agent {
            name: "Unnamed Agent".to_string(),
            features: Vec::default(),
            state: AgentState::Stopped,
            work_handles: Vec::default(),
            runtime: Arc::new(Runtime::new().unwrap()),
            hub: Arc::new(RwLock::new(Default::default()))
        };

        agt
    }
}

#[derive(Clone, Copy, Debug)]
pub enum AgentState {
    Ok,
    Stopped,
    Error,
}

#[derive(Clone, Copy, Debug)]
pub struct Schedule {}

#[derive(Clone, Debug)]
pub enum AgentCommand {
    Start,
    Schedule(Schedule, Workload),
    Stop,
}
#[derive(Debug, Clone)]
pub struct FeatureConfig {
    pub bind_address: [u8; 4],
    pub bind_port: u16,
    pub settings: HashMap<String, String>,
}

#[derive(Clone)]
pub struct AgentHandle {
    pub handle: Arc<RwLock<Agent>>,
    pub signal: Option<Sender<()>>
}

impl Debug for AgentHandle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_fmt(format_args!("Handle for Agent"))
    }
}

impl AgentHandle {
    pub fn new() -> AgentHandle {
        AgentHandle {
            handle: Arc::new(RwLock::new(Agent::default())),
            signal:None
        }
    }

    pub fn with_read<F>(&self, f: F) where F : FnOnce(&RwLockReadGuard<Agent>) + Sync + Send + 'static {
        let handle = self.handle.clone();

        let _ = futures::executor::block_on(tokio::task::spawn_blocking( move || {             
            let y = futures::executor::block_on(handle.read());
            f(&y);
            drop(y);
        } ));
    }

    pub fn with_write<F>(&self, f: F) where F : FnOnce(&RwLockWriteGuard<Agent>) + Sync + Send + 'static {
        let handle = self.handle.clone();

        let _ = futures::executor::block_on(tokio::task::spawn_blocking( move || {             
            let y = futures::executor::block_on(handle.write());
            f(&y);
            drop(y);
        } ));
    }
    pub async fn get_channel(&self, name: &str) -> (Receiver<AgentEvent>, Sender<AgentEvent>){
        let agent = self.handle.read().await;

        let chan = agent.hub.clone().write().await.get_or_create(name);

        drop(agent);

        (chan.receiver, chan.sender)
    }
    pub fn with_runtime(name: &str, runtime: Runtime) -> AgentHandle {
        AgentHandle {
            handle: Arc::new(RwLock::new(Agent::with_runtime(name, runtime))),
            signal: None
        }
    }
    pub async fn add_feature(&mut self, handle: FeatureHandle) -> Result<&mut AgentHandle> {
        self.handle.write().await.features.push(handle);

        Ok(self)
    }

    pub async fn start(&mut self) -> Receiver<()> {        
        let agent = self.handle.read().await;  
        
        for f in &agent.features {
            let feature_name =  f.handle.read().await.name();
            println!("Starting {}", feature_name);
            
            let (rx,tx) = self.get_channel("work").await; 
            let feature_handle = f.handle.clone();

            f.handle.write().await.init(self.clone());    

            agent.runtime.spawn(async move {                

                loop {
                    let message = rx
                        .recv()
                        .map_err(|err| anyhow!("Error receiving message: {}", err));

                    if let Ok(message) = message {   
                        println!("Writing event to feature");                                         
                        feature_handle.write().await.on_event(message.clone());                       
                        println!("Done writing event to feature");                       
                    }
                }
            });

            let _ = tx.send(AgentEvent::Started)
                .map_err(|err|  anyhow!("Error sending Agent Start event to {}: {}", feature_name,err)).unwrap();
                
        }
        
        drop(agent);

        self.handle.write().await.state = AgentState::Ok;

        let (rx, tx) = crossbeam_channel::bounded::<()>(1);

        self.signal = Some(rx);

        tx
    }
}
#[derive(Clone)]
pub struct FeatureHandle {
    handle: Arc<RwLock<dyn Feature>>,
}

impl FeatureHandle {
    pub fn new<T>(feature: T) -> FeatureHandle
    where
        T: Feature + 'static,
    {
        FeatureHandle {
            handle: Arc::new(RwLock::new(feature)),
        }
    }
}
pub struct Agent {
    name: String,
    runtime: Arc<Runtime>,
    features: Vec<FeatureHandle>,
    state: AgentState,
    hub: Arc<RwLock<Hub>>,
    work_handles: Vec<Arc<RwLock<WorkloadHandle>>>
}

impl Agent {
    pub fn new(name : &str) -> Agent {
        Agent {
            name: name.to_string(),
            runtime: Arc::new(Runtime::new().unwrap()),
            ..Default::default()
        }
    }
    pub fn with_runtime(name : &str, runtime: Runtime) -> Agent {
        Agent {
            name: name.to_string(),
            runtime: Arc::new(runtime),
            ..Default::default()
        }
    }
    pub async fn status(&self) -> AgentState {
        self.state
    }

    pub async fn command(&mut self, cmd: AgentCommand) {
        match cmd {
            AgentCommand::Start => {
                self.state = AgentState::Ok;
                let _ = self.hub.write().await.get_or_create("work").sender.send(AgentEvent::Started);
            }
            AgentCommand::Schedule(_, _) => {}
            AgentCommand::Stop => {
                self.state = AgentState::Stopped;
                let _ = self.hub.write().await.get_or_create("work").sender.send(AgentEvent::Stopped);
            }
        }
    }

    /// Run a workload and return a Sync + Send version of a handle
    /// That has information about that workload.
    /// 
    /// This will also add the workload handle to the agent's workload handle vector.
    
    fn run(&mut self, workload: Workload) -> Arc<RwLock<WorkloadHandle>> {
        let id = workload.id;

        let jh = self.runtime.clone().spawn(async move {           
              println!("Running workload.");
              workload.run().await 
        });

        let work_handle = Arc::new(RwLock::new(WorkloadHandle {
            id: id,
            join_handle: Some(jh),
            status: work::WorkloadStatus::Running,
            ..Default::default()
        }));

        self.work_handles.push(work_handle.clone());

        work_handle
    }
}
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct WorkloadData {
    pub result: String
}
pub enum AgentMessage {
    Stop,
    Start,
    Do(&'static str),
}

pub trait Feature: Send + Sync {
    //type Feature;
    fn init(&mut self, agent: AgentHandle);
    fn on_event(&mut self, event: AgentEvent);
    fn name(&self) -> String;
}
