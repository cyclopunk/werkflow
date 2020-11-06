use std::path::Path;
use bollard::{ClientVersion, Docker};
use anyhow::{Result};
use bollard::container::{Config, CreateContainerOptions, StartContainerOptions};
use bollard::image::CreateImageOptions;


use futures_util::stream::{TryStreamExt};

const DEFAULT_TIMEOUT : u64 = 60;
pub const API_DEFAULT_VERSION: &ClientVersion = &ClientVersion {
    major_version: 1,
    minor_version: 40,
};

pub struct ContainerService {
    docker: Docker
}

impl ContainerService {
    async fn connect_with_ssl(url: &str, ssl_key: &Path, ssl_cert: &Path, ssl_ca: &Path) -> ContainerService {
        ContainerService {
            docker: Docker::connect_with_ssl(url, ssl_key,ssl_cert, ssl_ca, DEFAULT_TIMEOUT, API_DEFAULT_VERSION).unwrap()
        }
    }
    
    async fn connect_with_http(url: &str) -> ContainerService {
        ContainerService {
            docker: Docker::connect_with_http(url, DEFAULT_TIMEOUT, API_DEFAULT_VERSION).unwrap()
        }
    }
    async fn create_and_start_container (&self, container_name: &str, image_name : &str, env: &[&str]) -> Result<()>{
        let config = Config {
            image: Some(image_name),
            env: Some(env.to_vec()),
            ..Default::default()
        };

        self.docker
        .create_image(
            Some(CreateImageOptions {
                from_image: image_name,
                ..Default::default()
            }),
            None,
            None,
        )
        .try_collect::<Vec<_>>()
        .await?;

        self.docker
            .create_container(
                Some(CreateContainerOptions { name: container_name }),
                config,
            )
            .await?;

        self.docker
            .start_container(container_name, None::<StartContainerOptions<String>>)
            .await?;

        Ok(())
    }
}