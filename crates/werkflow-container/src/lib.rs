use bollard::{image::*, container::*, Docker};
use anyhow::{anyhow, Result}
use bollard::container::{Config, CreateContainerOptions, LogsOptions, StartContainerOptions};
use bollard::image::CreateImageOptions;
use bollard::models::*;
use bollard::Docker;

use futures_util::stream::{TryStreamExt, select};

async fn create_and_start_container (name: &str, image_name : &str) -> Result<()>{

    #[cfg(unix)]
    let docker = Docker::connect_with_unix_defaults().unwrap();
    #[cfg(windows)]
    let docker = Docker::connect_with_named_pipe_defaults().unwrap();

    &docker
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

    &docker
        .create_container(
            Some(CreateContainerOptions { name: name }),
            zookeeper_config,
        )
        .await?;

    &docker
        .start_container(name, None::<StartContainerOptions<String>>)
        .await?;

    Ok(())
}