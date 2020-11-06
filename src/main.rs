use werkflow_config::ConfigSource;
use anyhow::Result;
use werkflow_agents::{AgentController, FeatureConfig, web::WebFeature};
use serde::{Serialize, Deserialize};
#[derive(Debug, Serialize, Deserialize)]
struct AgentConfig {
    name : String
}
#[tokio::main]
async fn main() -> Result<()> {
    let config : AgentConfig = werkflow_config::read_config(ConfigSource::File("werkflow.toml".into())).await?;

    let mut agent = AgentController::new(&config.name);
        
    agent
        .add_feature(WebFeature::new(FeatureConfig {
            bind_address: [127, 0, 0, 1],
            bind_port: 3030,
            settings: Default::default(),
        })).await?;

    Ok(())
}
