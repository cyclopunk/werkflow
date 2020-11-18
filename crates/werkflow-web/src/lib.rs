use handlebars::Handlebars;
use notify::DebouncedEvent;
use rhtml::Library;
use std::{fs, net::Ipv4Addr, sync::Arc, path::Path};
use tokio::sync::RwLock;
use werkflow_agents::cfg::ConfigDefinition;
use werkflow_agents::prom::{self, register_custom_metrics};
use werkflow_scripting::state::HostState;
use async_trait::async_trait;


use log::{info, warn};
use tokio::sync::oneshot::{self, Sender};

//use handlebars::Handlebars;

use warp::Filter;

use werkflow_agents::{comm::AgentEvent, AgentController, Feature, FeatureHandle};

use self::filters::agent_status;

use serde::{Deserialize, Serialize};

pub mod model;

mod filters;
mod handlers;
mod rhtml;

pub struct WebFeature {
    config: WebConfiguration,
    shutdown: Option<Sender<()>>,
    agent: Option<AgentController>,
}

impl<'a> WebFeature {
    pub fn new(config: impl ConfigDefinition + Serialize) -> FeatureHandle {
        Arc::new(RwLock::new(WebFeature {
            config: WebConfiguration::merge(config).expect("To merge configs"),
            shutdown: None,
            agent: None,
        }))
    }
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct WebConfiguration {
    pub bind_address: Ipv4Addr,
    pub port: u16,
    pub tls: Option<TlsConfiguration>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct TlsConfiguration {
    pub private_key_path: String,
    pub certificate_path: String,
}

impl Default for WebConfiguration {
    fn default() -> Self {
        WebConfiguration {
            bind_address: "127.0.0.1".parse().unwrap(),
            port: 3030,
            tls: Some(TlsConfiguration {
                private_key_path: "config/agent.key".into(),
                certificate_path: "config/agent.crt".into(),
            }),
        }
    }
}

impl ConfigDefinition for WebConfiguration {}

impl ConfigDefinition for TlsConfiguration {}

#[async_trait]
impl Feature for WebFeature {
    async fn init(&mut self, agent: AgentController) {
        self.agent = Some(agent);
        if let Some(_) = self.shutdown {
            return;
        }

        info!("Starting the web service");

        let state = Arc::new(RwLock::new(HostState::new()));
        let controller = self.agent.clone().unwrap();
        let config = self.config.clone();
        let (tx, rx) = oneshot::channel();

        self.shutdown = Some(tx);

        register_custom_metrics();

        // Use a log wrapper to add metrics to all of the calls.
        let log = warp::log::custom(|info| {
            prom::INCOMING_REQUESTS.inc();
            prom::RESPONSE_CODE_COLLECTOR
                .with_label_values(&[
                    "production",
                    &info.status().as_str(),
                    &info.method().to_string(),
                ])
                .inc();
        });                

        let (watcher, rx2, library) = Library::watch_directory("./templates")
            .await
            .expect("template library to be created");

        let threadsafe_lib = Arc::new(RwLock::new(library));

        let api = agent_status(controller.clone())            
            .or(filters::stop_agent(controller.clone()))
            .or(filters::start_agent(controller.clone()))
            .or(filters::start_job(controller.clone()))
            .or(filters::list_jobs(controller.clone()))
            .or(filters::templates(controller.clone(), state.clone(), threadsafe_lib.clone()))
            .or(filters::metrics())
            .with(log);

        let server = if let Some(tls) = config.tls {
            warp::serve(api)
                .tls()
                .key_path(tls.private_key_path)
                .cert_path(tls.certificate_path)
        } else {
            // Insecure stuff sucks
            panic!("Web feature is not supported without TLS.")
        };

        info!(
            "Spawning a webserver on {:?} {:?}",
            config.bind_address, config.port
        );

        let (_, srv) =
            server.bind_with_graceful_shutdown((config.bind_address, config.port), async {
                rx.await.ok();
            });
        let rt = &self.agent.as_ref().unwrap().agent.read().await.runtime;

        rt.spawn(async move {
            // move the watcher here so the channel stays alive.
            let watcher = watcher;
            let lib = threadsafe_lib.clone();
            loop {
                let x = rx2.recv();
                match x {
                    Ok(event) => {
                        match (event) {
                            DebouncedEvent::NoticeWrite(file) | DebouncedEvent::Write(file) => {
                                let mut writer = lib.write().await;

                                writer.update_from_file(file).await;
                            }
                            DebouncedEvent::NoticeRemove(_) => {}
                            DebouncedEvent::Create(_) => {}
                            DebouncedEvent::Chmod(_) => {}
                            DebouncedEvent::Remove(file) => {
                                let name = file.as_path()
                                    .file_stem()
                                    .unwrap()
                                    .to_string_lossy();
                                
                                let mut writer = lib.write().await;

                                writer.remove(name.as_ref());
                            }
                            DebouncedEvent::Rename(from_path, to_path) => {
                                let from_name = from_path.as_path()
                                .file_stem()
                                .unwrap()
                                .to_string_lossy();

                                let to_name = to_path.as_path()
                                .file_stem()
                                .unwrap()
                                .to_string_lossy();

                                let mut writer = lib.write().await;

                                writer.rename(from_name.as_ref(), 
                                to_name.as_ref());
                            }
                            DebouncedEvent::Rescan => {}
                            DebouncedEvent::Error(err, path) => {
                                warn!("Error scanning file: {} for path {:?}", err, path);
                            }
                        }
                    },
                    Err(e) => {
                        warn!("watch error: {}", e.to_string());
                        break;
                    },
                 }
            }
        });
        rt.spawn(srv);        
    }

    fn name(&self) -> String {
        return format!("Web Feature (running on port {})", self.config.port).to_string();
    }

    async fn on_event(&mut self, event: AgentEvent) {
        match event {
            AgentEvent::Started => {
                info!("Got started signal");                
            }
            AgentEvent::Stopped => {

                info!("Got stop signal")
            }
            AgentEvent::PayloadReceived(_payload) => {
                // todo
            }
            AgentEvent::WorkStarted(_workload) => {
                // todo
            }
            AgentEvent::WorkErrored(_err) => {
                // todo
            }
            AgentEvent::WorkComplete(_result) => {
                // todo
            }
        }
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use std::time::Duration;

    use tokio::runtime::Builder;

    #[test]
    #[ignore]
    fn web_test() {
        std::env::set_var("RUST_LOG", "trace");

        pretty_env_logger::init();
        let runtime = Builder::new()
            .threaded_scheduler()
            .enable_all()
            .build()
            .unwrap();

        let handle = &runtime.handle().clone();

        let mut agent = AgentController::with_runtime("Test", runtime);

        handle.block_on(async move {
            let chan = agent
                .add_feature(WebFeature::new(WebConfiguration {
                    bind_address: "127.0.0.1".parse().unwrap(),
                    port: 3030,
                    tls: None,
                }))
                .await
                .start()
                .await;
            let signal = agent.signal.as_ref().unwrap().clone();

            handle.spawn(async move {
                let _time = tokio::time::interval(Duration::from_millis(10000));
                signal.send(()).unwrap();
            });
            chan.recv().unwrap();
        })
    }
}
