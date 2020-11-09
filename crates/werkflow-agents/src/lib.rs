use anyhow::{anyhow, Result};
use channels::AGENT_CHANNEL;
use comm::{AgentEvent, Hub};
use config::Config;

use crossbeam_channel::{Receiver, Sender};
use log::{debug, info};
use parking_lot::RwLock;
use parking_lot::RwLockReadGuard;
use parking_lot::RwLockWriteGuard;
use std::{collections::HashMap, fmt::Debug};
use std::{sync::Arc, time::Instant};
use threads::AsyncRunner;
use tokio::runtime::Builder;


use lazy_static::*;

use tokio::runtime::Runtime;

use serde::{Deserialize, Serialize};
use work::{Workload, WorkloadHandle};

pub mod cfg;
pub mod comm;
pub(crate) mod prom;
pub mod threads;
pub mod web;
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
            hub: Arc::new(RwLock::new(Hub::new())),
            statistics: Arc::new(RwLock::new(AgentStatistics::default())),
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
pub struct AgentController {
    agent: Arc<RwLock<Agent>>,
    signal: Option<Sender<()>>,
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
    pub fn with_read<F>(&self, closure: F)
    where
        F: FnOnce(&RwLockReadGuard<Agent>) + Sync + Send + 'static,
    {
        let handle = self.agent.clone();

        let _ = async_std::task::block_on(tokio::task::spawn_blocking(move || {
            let agent = handle.read();
            closure(&agent);
            drop(agent);
        }));
    }
    pub fn with_write<F>(&self, closure: F)
    where
        F: FnOnce(&mut RwLockWriteGuard<Agent>) + Sync + Send + 'static,
    {
        let handle = self.agent.clone();

        let _ = async_std::task::block_on(tokio::task::spawn_blocking(move || {
            let mut agent = handle.write();
            closure(&mut agent);
            drop(agent);
        }));
    }
    pub fn send(&self, event: AgentEvent) -> Result<()> {
        let agent = self.agent.read();
        let mut hub = agent.hub.write();
        let channel = hub.get_or_create(AGENT_CHANNEL);

        channel
            .sender
            .send(event)
            .map_err(|err| anyhow!("{}", err))?;

        Ok(())
    }

    pub fn get_channel(&self, name: &str) -> (Receiver<AgentEvent>, Sender<AgentEvent>) {
        let agent = self.agent.read();

        let chan = agent.hub.clone().write().get_or_create(name);

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

    pub fn add_feature(&mut self, handle: FeatureHandle) -> &mut AgentController {
        let mut agent = self.agent.write();

        agent.features.push(handle);

        drop(agent);

        self
    }

    pub async fn start(&mut self) -> Receiver<()> {
        let agent = self.agent.read();

        info!("Feature count: {}", agent.features.len());
        for f in &agent.features {
            let feature_name = f.handle.read().name();
            let (rx, tx) = self.get_channel(AGENT_CHANNEL);
            let feature_handle = f.handle.clone();

            f.handle.write().init(self.clone());

            println!("Spawning on runtime");

            agent.runtime.spawn(async move {
                info!("Spawning feature communication channel.");

                loop {
                    let message = rx
                        .recv()
                        .map_err(|err| anyhow!("Error receiving message: {}", err));

                    if let Ok(message) = message {
                        info!("Got AgentEvent {}", message);

                        let mut feature = feature_handle.write();

                        feature.on_event(message.clone());

                        debug!("Done writing event to feature");

                        drop(feature);
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

        self.agent.write().state = AgentState::Ok;

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

impl FeatureHandle {
    pub async fn with_write<F>(&self, callback: F)
    where
        F: FnOnce(&mut RwLockWriteGuard<dyn Feature>) + Sync + Send + 'static,
    {
        callback(&mut self.handle.write());
    }
    pub async fn with_read<F>(&self, callback: F)
    where
        F: FnOnce(&RwLockReadGuard<dyn Feature>) + Sync + Send + 'static,
    {
        callback(&self.handle.read());
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct AgentStatistics {
    jobs_ran: u32,
    successful_jobs: u32,
    failed_jobs: u32,
    total_runtime: u128,
}

pub struct Agent {
    name: String,
    features: Vec<FeatureHandle>,
    state: AgentState,
    statistics: Arc<RwLock<AgentStatistics>>,
    runtime: Runtime,
    hub: Arc<RwLock<Hub<AgentEvent>>>,
    work_handles: Vec<Arc<tokio::sync::RwLock<WorkloadHandle>>>,
}

impl Agent {
    fn new(name: &str) -> Agent {
        Agent {
            name: name.to_string(),
            ..Default::default()
        }
    }
    fn with_runtime(name: &str, runtime: Runtime) -> Agent {
        Agent {
            name: name.to_string(),
            features: Vec::default(),
            runtime: runtime,
            state: AgentState::Stopped,
            work_handles: Vec::default(),
            hub: Arc::new(RwLock::new(Hub::new())),
            statistics: Arc::new(RwLock::new(AgentStatistics::default())),
        }
    }
    fn status(&self) -> AgentState {
        self.state
    }

    pub fn command(&mut self, cmd: AgentCommand) -> Result<()> {
        let channel = self.hub.write().get_or_create(AGENT_CHANNEL);
        match cmd {
            AgentCommand::Start => {
                self.state = AgentState::Ok;
                channel.sender.send(AgentEvent::Started)?;
            }
            AgentCommand::Schedule(_, _) => {}
            AgentCommand::Stop => {
                self.state = AgentState::Stopped;
                channel.sender.send(AgentEvent::Stopped)?;
            }
        }

        Ok(())
    }

    /// Run a workload in the agent
    /// This will capture the statistics of the workload run and store it in
    /// the agent.
    pub fn run(&mut self, workload: Workload) -> Arc<tokio::sync::RwLock<WorkloadHandle>> {
        let id = workload.id;

        self.statistics.write().jobs_ran += 1;

        let stats = self.statistics.clone();

        let jh = self.runtime.spawn(async move {
            info!("Running workload {}.", id);

            let start = Instant::now();

            let result = workload.run().await;

            let duration = start.elapsed();

            stats.write().total_runtime += duration.as_millis();

            match result {
                Ok(wl) => {
                    stats.write().successful_jobs += 1;

                    Ok(wl)
                }
                Err(_) => {
                    stats.write().failed_jobs += 1;
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
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct WorkloadData {
    pub result: String,
}
pub enum AgentMessage {
    Stop,
    Start,
    Do(&'static str),
}
pub trait Feature: Send + Sync {
    fn init(&mut self, agent: AgentController);
    fn on_event(&mut self, event: AgentEvent);
    fn name(&self) -> String;
}
