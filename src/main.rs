use anyhow::Result;
use clap::App;
use clap::Arg;
use log::info;
use reqwest::Url;
use serde::{Deserialize, Serialize};
use std::{net::Ipv4Addr, time::Duration};
use tokio::runtime::Builder;



use werkflow_agents::{
    web::WebFeature, AgentController, FeatureConfig,
};
use werkflow_config::ConfigSource;
use werkflow_core::HttpAction;

#[derive(Debug, Serialize, Deserialize, Default)]
struct AgentConfig {
    name: String,
    number: u16,
    web: Option<WebConfig>,
}

#[derive(Debug, Serialize, Deserialize)]
struct WebConfig {
    bind_address: String,
    port: u16,
}
fn main() -> Result<()> {
    let runtime = Builder::new()
        .threaded_scheduler()
        .enable_all()
        .build()
        .unwrap();

    let handle = runtime.handle().clone();

    handle.block_on(async move {
        let matches = App::new("Werkflow Agent")
            .version("1.0")
            .author("Adam Shaw <discourse@gmail.com>")
            .about("Workflow swiss-army knife")
            .arg(
                Arg::with_name("config")
                    .value_name("URL/FILENAME")
                    .help("URL or Filename for the agent configuration.")
                    .takes_value(true),
            )
            .arg(
                Arg::with_name("log")
                    .short("l")
                    .long("log-level")
                    .value_name("LOGLEVEL")
                    .help("trace, debug, info, warn")
                    .takes_value(true),
            )
            .get_matches();

        std::env::set_var("RUST_LOG", matches.value_of("log").unwrap_or("info"));
        pretty_env_logger::init();

        let config_name = matches
            .value_of("config")
            .unwrap_or("config/werkflow.toml")
            .clone();

        let config: AgentConfig = if let Ok(_) = config_name.parse::<Url>() {
            werkflow_config::read_config(ConfigSource::Http(HttpAction::Get(
                config_name.to_string(),
            )))
            .await
            .unwrap()
        } else {
            werkflow_config::read_config(ConfigSource::File(config_name.to_string()))
                .await
                .unwrap()
        };

        if let Some(web_config) = config.web {
            let mut channels = Vec::new();
            for i in 0..config.number {
                info!("Starting agent {}", i);
                let runtime = Builder::new()
                    .threaded_scheduler()
                    .enable_all()
                    .build()
                    .unwrap();

                let mut agent_c =
                    AgentController::with_runtime(&format!("{} - {}", &config.name, i), runtime);

                agent_c.add_feature(WebFeature::new(FeatureConfig {
                    bind_address: web_config
                        .bind_address
                        .parse::<Ipv4Addr>()
                        .unwrap()
                        .octets(),
                    bind_port: web_config.port + i,
                    settings: Default::default(),
                }));

                channels.push(agent_c.start().await)
            }
        }

        loop {
            std::thread::sleep(Duration::from_secs(5));
        }
    });

    Ok(())
}
