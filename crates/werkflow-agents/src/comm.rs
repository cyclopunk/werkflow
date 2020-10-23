use core::fmt::Display;
use std::collections::HashMap;
use crate::{AgentHandle, WorkloadData, work::{Workload, WorkloadHandle}};
use anyhow::Error;
use crossbeam_channel::{Receiver, Sender, unbounded};

pub enum AgentEvent {
    Started,
    Stopped,
    PayloadReceived(String),
    WorkStarted(Workload),
    WorkErrored(String),
    WorkComplete(WorkloadData)
}

impl Display for AgentEvent {    
    fn fmt(&self, fmt: &mut std::fmt::Formatter<'_>) -> std::result::Result<(), std::fmt::Error> { 
        match self {
            AgentEvent::Started => fmt.write_fmt(format_args!("Agent Started")),
            AgentEvent::Stopped => fmt.write_fmt(format_args!("Agent Stopped")),
            AgentEvent::PayloadReceived(_) => fmt.write_fmt(format_args!("Payload Received")),
            AgentEvent::WorkStarted(s) => fmt.write_fmt(format_args!("Work started id: {}, script: {:?}", s.id, s.script)),
            AgentEvent::WorkErrored(err_string) => fmt.write_fmt(format_args!("Workload Error: {}", err_string)),
            AgentEvent::WorkComplete(complete) => fmt.write_fmt(format_args!("Workload Complete: {:?}", complete))
        }
    }
}
#[derive(Clone)]
pub struct ChannelPair {
    pub name : String,
    pub sender : Sender<AgentEvent>,
    pub receiver: Receiver<AgentEvent>
}


#[derive(Default)]
pub struct Hub {
    pub channels: HashMap<String, ChannelPair>
}


impl Hub {
    pub fn new_channel(&mut self, name: &str) -> ChannelPair {
        let (s, r) = unbounded::<AgentEvent>();
        
        let channel_pair = ChannelPair {
            name: name.to_string(),
            sender: s,
            receiver: r
        };

        self.channels.insert(name.to_string(), channel_pair.clone());

        channel_pair
    }

    pub fn get_or_create(&mut self, name: &str) -> ChannelPair {
        if let Some(chan) = self.channels.get(&name.to_string()) {
            chan.clone()
        } else {
            self.new_channel(name)
        }
    }
}