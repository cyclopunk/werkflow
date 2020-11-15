use bollard::models::{HostConfig, PortBinding, PortMap};
use anyhow::Result;
use bollard::container::{Config, CreateContainerOptions, StartContainerOptions};
use bollard::image::CreateImageOptions;
use bollard::{ClientVersion, Docker};
use std::{collections::HashMap, path::Path};

use futures_util::stream::TryStreamExt;

const DEFAULT_TIMEOUT: u64 = 60;
pub const API_DEFAULT_VERSION: &ClientVersion = &ClientVersion {
    major_version: 1,
    minor_version: 40,
};

pub struct ContainerService {
    docker: Docker,
}

impl ContainerService {
    pub async fn connect_with_ssl(
        url: &str,
        ssl_key: &Path,
        ssl_cert: &Path,
        ssl_ca: &Path,
    ) -> ContainerService {
        ContainerService {
            docker: Docker::connect_with_ssl(
                url,
                ssl_key,
                ssl_cert,
                ssl_ca,
                DEFAULT_TIMEOUT,
                API_DEFAULT_VERSION,
            )
            .unwrap(),
        }
    }

    pub async fn connect_with_http(url: &str) -> ContainerService {
        ContainerService {
            docker: Docker::connect_with_http(url, DEFAULT_TIMEOUT, API_DEFAULT_VERSION).unwrap(),
        }
    }
    pub async fn default_connect() -> ContainerService {
        ContainerService {
            docker: Docker::connect_with_named_pipe_defaults().expect("could not connect to docker")
        }
    }
    pub async fn start_container(
        &self,
        container_name: String,
        image_name: String,
        env: &[String],
        port_forward: HashMap<String, String> 
    ) -> Result<()> {
        
        let mut port_map : PortMap = HashMap::new();

        for (k,v) in port_forward {
            let mut bindings: Vec<PortBinding> = Vec::new();

            bindings.push(PortBinding {
                host_ip: Some("127.0.0.1".into()),
                host_port: Some(v)
            });

            port_map.insert(k, Some(bindings));
        }
        let mut host_config =  HostConfig::default();
        host_config.port_bindings = Some(port_map);

        let config = Config {
            image: Some(image_name.clone()),
            env: Some(env.to_vec()),
            host_config: Some(host_config),
            ..Default::default()
        };

        self.docker
            .create_container(
                Some(CreateContainerOptions {
                    name: container_name.clone(),
                }),
                config,
            )
            .await?;

        self.docker
            .start_container(&container_name, None::<StartContainerOptions<String>>)
            .await?;

        Ok(())
    }
    pub async fn create_and_start_container(
        &self,
        container_name: String,
        image_name: String,
        env: &[String],
    ) -> Result<()> {
        let config = Config {
            image: Some(image_name.clone()),
            env: Some(env.to_vec()),
            ..Default::default()
        };

        self.docker
            .create_image(
                Some(CreateImageOptions {
                    from_image: image_name.clone(),
                    ..Default::default()
                }),
                None,
                None,
            )
            .try_collect::<Vec<_>>()
            .await?;

        self.docker
            .create_container(
                Some(CreateContainerOptions {
                    name: container_name.clone(),
                }),
                config,
            )
            .await?;

        self.docker
            .start_container(&container_name, None::<StartContainerOptions<String>>)
            .await?;

        Ok(())
    }
}
