use std::path::Path;

use anyhow::{anyhow, Result};
use serde::{Deserialize, Serialize};
use werkflow_core::sec::DnsProvider;

/// Trait that will allow configurations to be easily
/// created, structured and shared without knowing the schema of the config.

pub trait ConfigDefinition {
    fn from_file<T>(path: &Path) -> Result<T>
    where
        for<'d> T: ConfigDefinition + Deserialize<'d>,
        Self: Sized,
    {
        werkflow_config::blocking::from_file(path)
    }
    fn get<T>(&self, key: &str) -> T
    where
        for<'d> T: Deserialize<'d>,
        Self: Serialize,
    {
        self.try_get(key).unwrap()
    }
    fn try_get<T>(&self, key: &str) -> Result<T>
    where
        for<'d> T: Deserialize<'d>,
        Self: Serialize,
    {
        let map = serde_json::to_value(self).unwrap();

        let v = map.get(key).unwrap();

        serde_json::from_str(&serde_json::to_string(v).unwrap())
            .map_err(|err| anyhow!("Could not deserliaze key {} from config {}", key, err))
    }

    fn merge<'d, T>(config: impl Serialize) -> Result<T>
    where
        for<'a> T: Deserialize<'a>,
    {
        let map = serde_json::to_value(config).unwrap();

        serde_json::from_value(map).map_err(|_err| anyhow!("Could not merge configs"))
    }
}

#[derive( Clone, Serialize, Deserialize)]
pub struct DnsConfiguration {
    pub api_key: String,
    pub provider: DnsProvider,

}

impl ConfigDefinition for DnsConfiguration {}

#[derive(Default, Clone, Serialize, Deserialize)]
pub struct AgentConfiguration {
    pub dns: Option<DnsConfiguration>,
}

#[test]
fn test_config_get() {
    let dns = DnsConfiguration {
        api_key: "test".to_string(),
        provider: DnsProvider::Cloudflare
    };

    assert_eq!("test", dns.get::<String>("api_key"));
}
