use anyhow::*;
use redis::{Connection, RedisResult};

fn connect(connection_string : &str) -> RedisResult<Connection> {
    let client = redis::Client::open(connection_string)?;

    let con = client.get_connection()?;
    /* do something here */

    Ok(con)
}

#[cfg(test)]
mod tests {
    use werkflow_config::read_config;
    use crate::database::CacheConfig;

    use redis::Commands;
    use super::*;
    #[tokio::test(threaded_scheduler)]
    async fn test_read_config() -> Result<(), anyhow::Error> {

        let config_file = r#"        
        enabled=true
        host="redis://127.0.0.1:6379/"
        "#;

        let cfg : CacheConfig = read_config(werkflow_config::ConfigSource::String(config_file.into())).await?;
        

        let mut x = connect(&cfg.host)?;

        x.set("test", 42)?;

        assert_eq!(x.get::<&str, i8>("test").unwrap(), 42);

        Ok(())
    }
}