use serde::{Serialize, Deserialize};

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