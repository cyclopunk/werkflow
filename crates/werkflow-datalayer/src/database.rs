

use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Default, Clone)]
pub struct DatabaseConfig {
    pub source_type: String,
    pub host: String,
    pub username: Option<String>,
    pub password: Option<String>,
    pub port: i32,
}
#[derive(Serialize, Deserialize, Default, Clone)]
pub struct CacheConfig {
    pub host: String,
    pub enabled: bool,
}
#[derive(Serialize, Deserialize, Default, Clone)]
pub struct DistributedConfig {
    pub nodes: Option<Vec<String>>,
    pub init_script: Option<String>,
}
#[derive(Serialize, Deserialize, Default, Clone)]
pub struct DataSourceConfig {
    pub database: DatabaseConfig,
    pub cache: CacheConfig,
    pub distributed: Option<DistributedConfig>,
}

#[cfg(test)]
mod tests {
        use crate::database::DatabaseConfig;
use werkflow_config::{ConfigSource, read_config};

    use super::DataSourceConfig;
    #[tokio::test(threaded_scheduler)]
    async fn test_read_config() -> Result<(), anyhow::Error> {
        let config_file = r#"
        [cache]
        enabled=true
        host="localhost:"

        [database]
        source_type="cassandra"
        host="localhost"
        port=9042
        username=""
        password=""

        [distribtued]
        nodes=[]
        "#
        .trim_start_matches(" ");
        let cfg : DataSourceConfig = read_config(ConfigSource::String(config_file.into())).await?;
        

        Ok(())
    }
}
