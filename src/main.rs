use crate::model::AgentConfig;
use anyhow::Result;
use clap::App;
use clap::Arg;
use log::info;
use reqwest::Url;
use std::{path::Path, time::Duration};
use tokio::runtime::Builder;
use werkflow_core::sec::ZoneRecord;
use werkflow_core::sec::{Authentication, DnsControllerClient, DnsProvider};
use werkflow_core::sec::{CertificateProvider, Zone};
use werkflow_web::*;

use werkflow_agents::AgentController;
use werkflow_config::ConfigSource;
use werkflow_core::HttpAction;

mod model;

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
                Arg::with_name("name")
                    .short("n")
                    .long("name")
                    .value_name("AGENTNAME")
                    .help("Name of the agent")
                    .takes_value(true),
            )
            .arg(
                Arg::with_name("domain")
                    .short("d")
                    .long("domain")
                    .value_name("DOMAINNAME")
                    .help("Domain name")
                    .takes_value(true),
            )
            .arg(
                Arg::with_name("dns-api-key")
                    .short("k")
                    .long("dns-key")
                    .value_name("KEY")
                    .help("DNS API Key")
                    .takes_value(true),
            )
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

        // if the config_name is a url, use the http/https as a config source.
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
        let bind_address = {
            config
                .web
                .as_ref()
                .map(|c| c.bind_address)
                .map(|c| c.to_string())
                .unwrap_or("127.0.0.1".into())
        };

        let configure_tls = config
            .web
            .as_ref()
            .map(|o| match o.tls.as_ref() {
                Some(conf) => conf,
                None => {
                    panic!("Missing TLS configuration. Werkflow agents require this configuration.")
                }
            })
            .expect("to have a tls configuration");

        // if certificates for this agent don't exist, create them with the agent_name + dns_config.domain.
        // This uses the CertificateProvider (which is a abstraction for LetsEncrypt) and manages the entire
        // certificate creation through that.
        if !Path::new(&configure_tls.certificate_path).exists() {
            if let Some(dns_config) = config.dns {
                
                let name = if let Some(name) = matches.value_of("name") {
                    name.to_string()
                } else {
                    config.name.to_string()
                };

                let domain = if let Some(domain) = matches.value_of("domain") {
                    domain.to_string()
                } else {
                    dns_config.domain.to_string()
                };

                let key = if let Some(dns_key) = matches.value_of("dns-api-key") {
                    dns_key.to_string()
                } else {
                    dns_config.api_key.to_string()
                };

                let fqdn = format!("{}.{}", name, domain);

                let zone = Zone::ByName(domain);

                let provider = DnsProvider::Cloudflare
                    .new(Authentication::ApiToken(key));

                provider
                    .add_or_replace(&zone, &ZoneRecord::A(fqdn.clone(), bind_address))
                    .await
                    .expect("to add a local address");

                let mut p = CertificateProvider::new("discourse@gmail.com")
                    .await
                    .expect("Could not create certificate provider.");

                let domains = vec![fqdn.into()];

                let certs = p
                    .order_with_dns(provider, &zone, domains.clone())
                    .await
                    .expect("Could order with DNS.");

                for (_i, cert) in certs.iter().enumerate() {
                    cert.save_signed_certificate("config/agent.crt")
                        .await
                        .expect("could not save signed certificate");
                    cert.save_private_key("config/agent.key")
                        .await
                        .expect("Could not save private key");
                }
            }
        }

        if let Some(web_config) = config.web.clone() {
            let mut channels = Vec::new();
            // it's currently possible to spin up multiple agents on a single instance
            // this will read the AgentConfig for a number and spin up that many agents.
            // Each agent will have their own tokio runtime. Agents will only communicate with eachother
            // via network channels.
            for i in 0..config.number {
                info!("Starting agent {}", i);
                let runtime = Builder::new()
                    .threaded_scheduler()
                    .enable_all()
                    .build()
                    .unwrap();

                let mut agent_c =
                    AgentController::with_runtime(&format!("{} - {}", &config.name, i), runtime);

                agent_c
                    .add_feature(WebFeature::new(web_config.clone()))
                    .await;

                channels.push(agent_c.start().await)
            }
        }

        loop {
            std::thread::sleep(Duration::from_secs(5));
        }
    });

    Ok(())
}
