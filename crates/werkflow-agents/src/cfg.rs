
use serde::{Deserialize, Serialize};
#[derive(Default, Clone, Serialize, Deserialize)]
pub struct DnsConfiguration {
    pub api_key: String
}
#[derive(Default, Clone, Serialize, Deserialize)]
pub struct AgentConfiguration {
    pub dns : Option<DnsConfiguration>
}