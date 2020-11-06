use crate::{work::Workload, WorkloadData};
use core::fmt::Display;
use std::collections::HashMap;

use crossbeam_channel::{unbounded, Receiver, Sender};

#[derive(Clone)]
pub enum AgentEvent {
    Started,
    Stopped,
    PayloadReceived(String),
    WorkStarted(Workload),
    WorkErrored(String),
    WorkComplete(WorkloadData),
}

impl Display for AgentEvent {
    fn fmt(&self, fmt: &mut std::fmt::Formatter<'_>) -> std::result::Result<(), std::fmt::Error> {
        match self {
            AgentEvent::Started => fmt.write_fmt(format_args!("Agent Started")),
            AgentEvent::Stopped => fmt.write_fmt(format_args!("Agent Stopped")),
            AgentEvent::PayloadReceived(_) => fmt.write_fmt(format_args!("Payload Received")),
            AgentEvent::WorkStarted(s) => fmt.write_fmt(format_args!(
                "Work started id: {}, script: {:?}",
                s.id, s.script
            )),
            AgentEvent::WorkErrored(err_string) => {
                fmt.write_fmt(format_args!("Workload Error: {}", err_string))
            }
            AgentEvent::WorkComplete(complete) => {
                fmt.write_fmt(format_args!("Workload Complete: {:?}", complete))
            }
        }
    }
}
#[derive(Clone)]
pub struct ChannelPair<T>
where
    T: Send + Sync + Clone,
{
    pub name: String,
    pub sender: Sender<T>,
    pub receiver: Receiver<T>,
}

#[derive(Default)]
pub struct Hub<T>
where
    T: Send + Sync + Clone,
{
    pub channels: HashMap<String, ChannelPair<T>>,
}

impl<T> Hub<T>
where
    T: Send + Sync + Clone,
{
    pub fn new() -> Hub<T> {
        Hub {
            channels: HashMap::new(),
        }
    }
    pub fn new_channel(&mut self, name: &str) -> ChannelPair<T> {
        let (s, r) = unbounded::<T>();

        let channel_pair = ChannelPair::<T> {
            name: name.to_string(),
            sender: s,
            receiver: r,
        };

        self.channels.insert(name.to_string(), channel_pair.clone());

        channel_pair
    }

    pub fn get_or_create(&mut self, name: &str) -> ChannelPair<T> {
        if let Some(chan) = self.channels.get(&name.to_string()) {
            chan.clone()
        } else {
            self.new_channel(name)
        }
    }
}
