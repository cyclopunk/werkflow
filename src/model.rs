
use werkflow_web::WebConfiguration;
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize, Default)]
pub struct AgentConfig {
    pub name: String,
    pub number: u16,
    pub web: Option<WebConfiguration>,
    pub dns: Option<DnsConfiguration>,
}

#[derive(Debug, Serialize, Deserialize, Default)]
pub struct DnsConfiguration {
    pub api_key: String,
    pub domain: String,
}