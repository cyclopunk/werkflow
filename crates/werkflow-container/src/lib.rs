use bollard::Docker;
use anyhow::{anyhow, Result}

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