use tokio::runtime::Runtime;
use werkflow_agents::{web::WebFeature, Agent, AgentHandle, Feature, FeatureConfig};
 
fn main() {
    let runtime = Runtime::new().unwrap();
    let handle = runtime.handle().clone();

    let mut agent = AgentHandle::with_runtime("Default Agent", runtime);

    handle.block_on(async move {
        agent
        .add_feature(WebFeature::new(FeatureConfig {
            bind_address: [127, 0, 0, 1],
            bind_port: 3030,
            settings: Default::default(),
        })).await
            .start().await
    })
}
