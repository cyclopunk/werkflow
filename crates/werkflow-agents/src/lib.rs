use tokio::time::delay_for;
use core::time::Duration;
use anyhow::{anyhow, Result};
use channels::AGENT_CHANNEL;
use comm::{AgentEvent, Hub};
use config::Config;
use tokio::sync::RwLock;
use tokio::sync::RwLockReadGuard;
use tokio::sync::RwLockWriteGuard;

use crossbeam_channel::{Receiver, Sender};
use log::{debug, info};
use std::{collections::HashMap, fmt::Debug};
use std::{sync::Arc, time::Instant};
use threads::AsyncRunner;
use tokio::runtime::Builder;

use lazy_static::*;

use tokio::runtime::Runtime;

use work::{Workload, WorkloadHandle, WorkloadStatus};

use async_trait::async_trait;
pub mod cfg;
pub mod comm;
pub mod plugins;
pub mod prom;
pub mod threads;
pub mod work;

pub mod channels {
    pub const AGENT_CHANNEL: &'static str = "Agent";
}

// Configuration

lazy_static! {
    static ref SETTINGS: RwLock<Config> = RwLock::new(Config::default());
}

impl<'a> Default for Agent {
    fn default() -> Agent {
        let agt = Agent {
            name: "Unnamed Agent".to_string(),
            features: Vec::default(),
            runtime: Runtime::new().unwrap(),
            state: AgentState::Stopped,
            work_handles: Vec::default(),
            hub: Hub::new(),
        };

        agt
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
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
pub struct AgentController {
    pub agent: Arc<RwLock<Agent>>,
    pub signal: Option<Sender<()>>,
}

impl Debug for AgentController {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_fmt(format_args!("Handle for Agent"))
    }
}

impl AgentController {
    pub fn new(name: &str) -> AgentController {
        let runtime = Builder::new()
            .threaded_scheduler()
            .enable_all()
            .build()
            .unwrap();
        let agent = Agent::with_runtime(name, runtime);
        AgentController {
            agent: Arc::new(RwLock::new(agent)),
            signal: None,
        }
    }
    pub async fn with_read<F>(&self, closure: F)
    where
        F: FnOnce(&RwLockReadGuard<Agent>) + Sync + Send + 'static,
    {
        let handle = self.agent.clone();

        let agent = handle.read().await;

        closure(&agent);
    }
    pub async fn with_write<F>(&self, closure: F)
    where
        F: FnOnce(&mut RwLockWriteGuard<Agent>) + Sync + Send + 'static,
    {
        let handle = self.agent.clone();

        let mut agent = handle.write().await;
        closure(&mut agent);
        drop(agent);
    }
    pub async fn send(&self, event: AgentEvent) -> Result<()> {
        let agent = self.agent.read().await;
        let mut hub = agent.hub.write().await;
        let channel = hub.get_or_create(AGENT_CHANNEL);

        channel
            .sender
            .send(event)
            .map_err(|err| anyhow!("{}", err))?;

        Ok(())
    }

    pub async fn get_channel(&self, name: &str) -> (Receiver<AgentEvent>, Sender<AgentEvent>) {
        let agent = self.agent.read().await;

        let chan = agent.hub.clone().write().await.get_or_create(name);

        drop(agent);

        (chan.receiver, chan.sender)
    }

    pub fn with_runtime(name: &str, runtime: Runtime) -> AgentController {
        let agent = Agent::with_runtime(name, runtime);
        AgentController {
            agent: Arc::new(RwLock::new(agent)),
            signal: None,
        }
    }

    pub async fn add_feature(&mut self, handle: FeatureHandle) -> &mut AgentController {
        let mut agent = self.agent.write().await;

        agent.features.push(handle);

        drop(agent);

        self
    }

    pub async fn start(&mut self) -> Receiver<()> {
        let agent = self.agent.read().await;

        info!("Feature count: {}", agent.features.len());
        for f in &agent.features {
            let feature_name = f.read().await.name();
            let (rx, tx) = self.get_channel(AGENT_CHANNEL).await;

            f.write().await.init(self.clone()).await;

            let feature_handle = f.clone();
            agent.runtime.spawn(async move {
                debug!("Spawning feature communication channel.");

                loop {
                    let fh = feature_handle.clone();
                    let message = rx
                        .recv()
                        .map_err(|err| anyhow!("Error receiving message: {}", err));

                    if let Ok(message) = message {
                        info!("Got AgentEvent {}", message);

                        let mut feature = fh.write().await;

                        feature.on_event(message.clone()).await;

                        debug!("Done writing event to feature");
                    }
                }
            });

            let _ = tx
                .send(AgentEvent::Started)
                .map_err(|err| {
                    anyhow!(
                        "Error sending Agent Start event to {}: {}",
                        feature_name,
                        err
                    )
                })
                .unwrap();
        }

        drop(agent);

        self.agent.write().await.state = AgentState::Ok;

        let (rx, tx) = crossbeam_channel::bounded::<()>(1);

        self.signal = Some(rx);

        tx
    }

    pub async fn schedule<T>(&self, interval : Duration, func : T) where T : Fn() -> () + Clone + Send + 'static {
        let handle = {
            let agent = self.agent.read().await;
            agent.runtime.handle().clone()
        };
        
        let f = func.clone();

        handle.spawn(async move {
            info!("Starting scheduled function");
            loop {
                f.clone()();
                delay_for(interval).await;
            }
        });
    }
}

/*
impl FeatureHandle {
    pub fn new<T>(feature: T) -> FeatureHandle
    where
        T: Feature + 'static,
    {
        FeatureHandle {
            handle: Arc::new(tokio::sync::RwLock::new(feature)),
        }
    }
}

impl<'a> FeatureHandle {
    pub async fn with_write<F>(&self, callback: F)
    where
        F: FnOnce(&mut RwLockWriteGuard<dyn Feature>) + Sync + Send + 'static,
    {
        callback(&mut self.handle.write().await);
    }
    pub async fn with_read<F>(&self, callback: F)
    where
        F: FnOnce(&RwLockReadGuard<dyn Feature>) + Sync + Send + 'static,
    {
        callback(&self.handle.read().await);
    }
}*/

type HubHandle = Arc<RwLock<Hub<AgentEvent>>>;
pub struct Agent {
    pub name: String,
    pub features: Vec<FeatureHandle>,
    pub state: AgentState,
    pub runtime: Runtime,
    pub hub: HubHandle,
    pub work_handles: Vec<Arc<tokio::sync::RwLock<WorkloadHandle>>>,
}

impl Agent {
    pub fn new(name: &str) -> Agent {
        Agent {
            name: name.to_string(),
            ..Default::default()
        }
    }
    pub fn with_runtime(name: &str, runtime: Runtime) -> Agent {
        Agent {
            name: name.to_string(),
            features: Vec::default(),
            runtime: runtime,
            state: AgentState::Stopped,
            work_handles: Vec::default(),
            hub: Hub::new(),
        }
    }
    pub fn status(&self) -> AgentState {
        self.state
    }

    /// Run a command on the agent.
    pub async fn command(&mut self, cmd: AgentCommand) {
        let channel = self.hub.write().await.get_or_create(AGENT_CHANNEL);
        match cmd {
            AgentCommand::Start => {
                self.state = AgentState::Ok;
                let work_handles = self.work_handles.clone();

                println!("Workload handles: {}", work_handles.len());
                // rerun defered workload handles
                for wl in work_handles {
                    let wl2 = wl.clone();

                    let (status, workload) = AsyncRunner::block_on(async move {
                        let wl = wl.read().await;
                        (wl.status.clone(), wl.workload.as_ref().unwrap().clone())
                    });

                    if status == WorkloadStatus::None {
                        self.run(workload);
                    }

                    AsyncRunner::block_on(async move {
                        let mut handle = wl2.write().await;
                        handle.status = WorkloadStatus::Complete;
                    });
                }
                channel.sender.send(AgentEvent::Started).unwrap();
            }
            AgentCommand::Schedule(_, _) => {}
            AgentCommand::Stop => {
                self.state = AgentState::Stopped;
                channel.sender.send(AgentEvent::Stopped).unwrap();
            }
        }
    }

    /// Run a workload in the agent
    /// This will capture the statistics of the workload run and store it in
    /// the agent.
    pub fn run(&mut self, workload: Workload) -> Arc<tokio::sync::RwLock<WorkloadHandle>> {
        if self.state == AgentState::Stopped {
            info!("Agent stopped, Not running workload {}. Work will be deferred until the agent starts.", workload.id);

            let work_handle = Arc::new(tokio::sync::RwLock::new(WorkloadHandle {
                id: workload.id,
                join_handle: None,
                status: WorkloadStatus::None,
                workload: Some(workload),
                ..Default::default()
            }));

            self.work_handles.push(work_handle.clone());

            return work_handle;
        }

        let id = workload.id;

        prom::WORKLOAD_START.inc();

        let jh = self.runtime.spawn(async move {
            info!("[Workload {}] Running.", id);

            let start = Instant::now();

            let result = workload.run().await;
            let mills = start.elapsed().as_millis();

            info!("[Workload {}] Duration: {}ms", id, mills as f64);

            prom::WORKLOAD_TOTAL_TIME.inc_by(mills as i64);
            crate::prom::WORKLOAD_TIME_COLLECTOR
                .with_label_values(&["processing_time"])
                .observe(mills as f64 / 1000.);

            match result {
                Ok(wl) => {
                    prom::WORKLOAD_COMPLETE.inc();

                    Ok(wl)
                }
                Err(_) => {
                    prom::WORKLOAD_ERROR.inc();

                    Err(anyhow!("Workload run failed."))
                }
            }
        });

        let work_handle = Arc::new(tokio::sync::RwLock::new(WorkloadHandle {
            id: id,
            join_handle: Some(jh),
            status: work::WorkloadStatus::Running,
            ..Default::default()
        }));

        self.work_handles.push(work_handle.clone());

        work_handle
    }
}

pub type FeatureHandle = Arc<tokio::sync::RwLock<dyn Feature>>;
#[async_trait]
pub trait Feature: Send + Sync {
    async fn init(&mut self, agent: AgentController);
    async fn on_event(&mut self, event: AgentEvent);
    fn name(&self) -> String;
}
