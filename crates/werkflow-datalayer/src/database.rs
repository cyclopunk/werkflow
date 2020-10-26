

use serde::{Deserialize, Serialize};



#[derive(Serialize, Deserialize, Default, Clone)]
pub struct DatabaseConfig {
    pub source_type: String,
    pub host: String,
    pub username: Option<String>,
    pub password: Option<String>,
    pub port: i32,
}
#[derive(Serialize, Deserialize, Default, Clone, Copy)]
pub struct CacheConfig {
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
        use werkflow_config::read_config;
    #[tokio::test(threaded_scheduler)]
    async fn test_read_config() -> Result<(), anyhow::Error> {
        let filename = "./tmp-config.toml";
        let config_file = r#"
        [cache]
        enabled=true

        [database]
        source_type="cassandra"
        host="localhost"
        port=9042
        "#
        .trim_start_matches(" ");

        std::fs::write(filename, config_file)?;

        let cfg = read_config(werkflow_config::ConfigSource::File(filename.into())).await?;

        std::fs::remove_file(filename)?;

        Ok(())
    }
}
