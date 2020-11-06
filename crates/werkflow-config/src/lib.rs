use anyhow::anyhow;
use config::*;
use serde::Deserialize;

use werkflow_core::HttpAction;

#[derive(Debug, Clone)]
pub enum ConfigSource {
    String(String),
    Http(HttpAction),
    File(String),
}

pub async fn read_config<T>(source: ConfigSource) -> Result<T, anyhow::Error>
where
    for<'de> T: Deserialize<'de>,
{
    match source {
        ConfigSource::String(file) => {
            let mut config = Config::new();

            config
                .merge(File::from_str(&file, FileFormat::Toml))
                .map_err(|err| anyhow!("Could get config rom string provided. {}", err))?;

            config.try_into::<T>().map_err(|err| {
                anyhow!(
                    "Could not coerce configuration into DataSourceConfig: {}",
                    err
                )
            })
        }
        ConfigSource::Http(url) => read_web_config::<T>(url.clone())
            .await
            .map_err(|err| anyhow!("Could not get config from the web. {}", err)),
        ConfigSource::File(filename) => {
            let mut config = Config::new();
            config
                .merge(File::with_name(&filename))
                .map_err(|err| anyhow!("Could not get config from file {}", err))?;
            config.try_into::<T>().map_err(|err| {
                anyhow!(
                    "Could not coerce configuration into DataSourceConfig: {}",
                    err
                )
            })
        }
    }
}

async fn read_web_config<T>(action: HttpAction) -> Result<T, anyhow::Error>
where
    for<'de> T: Deserialize<'de>,
{
    let client = reqwest::Client::new();

    let final_action = match action.clone() {
        HttpAction::Get(url) => client
            .get(&url)
            .header("User-Agent", "werkflow-agent/0.1.0"),
        HttpAction::Post(url) => client
            .post(&url)
            .header("User-Agent", "werkflow-agent/0.1.0"),
        HttpAction::GetWithHeaders(url, headers) => {
            let mut rb = client
                .get(&url)
                .header("User-Agent", "werkflow-agent/0.1.0");

            for (k, v) in &headers {
                rb = rb.header(k, v);
            }

            rb
        }
        HttpAction::PostWithHeaders(url, headers) => {
            let mut rb = client
                .post(&url)
                .header("User-Agent", "werkflow-agent/0.1.0");

            for (k, v) in &headers {
                rb = rb.header(k, v);
            }

            rb
        }
    };

    let src = final_action
        .send()
        .await
        .map_err(|err| anyhow!("Error contacting url {:?}, {}", action, err))?
        .text()
        .await
        .map_err(|err| anyhow!("Could not make the result text, {}", err))?;

    let mut config = Config::new();

    config
        .merge(File::from_str(&src, FileFormat::Toml))
        .map_err(|err| anyhow!("Could not create config from url {:?}: {}", action, err))?;

    config.try_into::<T>().map_err(|err| {
        anyhow!(
            "Could not coerce into a config object {:?}: {}",
            action,
            err
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::Serialize;

    #[derive(Serialize, Deserialize, Default, Clone)]
    pub struct DatabaseConfig {
        source_type: String,
        host: String,
        username: Option<String>,
        password: Option<String>,
        port: i32,
    }
    #[derive(Serialize, Deserialize, Default, Clone, Copy)]
    pub struct CacheConfig {
        enabled: bool,
    }

    #[derive(Serialize, Deserialize, Default, Clone)]
    pub struct DataSourceConfig {
        database: DatabaseConfig,
        cache: CacheConfig,
    }

    #[tokio::test(threaded_scheduler)]
    async fn test_read_config() -> Result<(), anyhow::Error> {
        use warp::Filter;

        let filename = "./tmp-config.toml";
        let config_file = r#"
        [cache]
        enabled=true

        [database]
        source_type="cassandra"
        host="localhost"
        port=9042

        [distributed]
        "#
        .trim_start_matches(" ");

        std::fs::write(filename, config_file)?;

        let cfg = read_config::<DataSourceConfig>(ConfigSource::File(filename.into())).await?;

        assert_eq!(cfg.database.host, "localhost");
        assert_eq!(cfg.database.port, 9042);
        assert_eq!(cfg.database.source_type, "cassandra");
        assert_eq!(cfg.cache.enabled, true);

        std::fs::remove_file(filename)?;
        // GET /hello/warp => 200 OK with body "Hello, warp!"
        let config = warp::path!("config").map(move || format!("{}", config_file));

        tokio::spawn(warp::serve(config).run(([127, 0, 0, 1], 5042)));

        let config_web_result = read_web_config::<DataSourceConfig>(HttpAction::Get(
            "http://localhost:5042/config".into(),
        ))
        .await
        .map_err(|err| anyhow!("Couldn't read config from web url: {}", err))
        .unwrap();

        assert_eq!(config_web_result.database.host, "localhost");
        assert_eq!(config_web_result.database.port, 9042);
        assert_eq!(config_web_result.database.source_type, "cassandra");
        assert_eq!(config_web_result.cache.enabled, true);

        Ok(())
    }
}
