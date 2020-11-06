use anyhow::Result;
use serde::{Deserialize, Serialize};
use werkflow_agents::{web::WebFeature, AgentController, FeatureConfig};
use werkflow_config::ConfigSource;
#[derive(Debug, Serialize, Deserialize)]
struct AgentConfig {
    name: String,
}
#[tokio::main]
async fn main() -> Result<()> {
    let config: AgentConfig =
        werkflow_config::read_config(ConfigSource::File("werkflow.toml".into())).await?;

    let mut agent = AgentController::new(&config.name);

    agent
        .add_feature(WebFeature::new(FeatureConfig {
            bind_address: [127, 0, 0, 1],
            bind_port: 3030,
            settings: Default::default(),
        }))
        .await?;

    Ok(())
}
