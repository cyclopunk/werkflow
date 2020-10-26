#![feature(type_alias_impl_trait)]

use cdrs::cluster::TcpConnectionPool;
use database::DataSourceConfig;
use werkflow_config::{read_config, ConfigSource};

use cdrs::types::prelude::TryFromRow;
use cdrs_helpers_derive::*;

use cdrs::types::from_cdrs::FromCDRSByName;

use anyhow::anyhow;
use cdrs::authenticators::NoneAuthenticator;
use cdrs::cluster::session::new as new_session;
use cdrs::cluster::{session::Session, NodeTcpConfig};
use cdrs::cluster::{ClusterTcpConfig, NodeTcpConfigBuilder};

use cdrs::load_balancing::RoundRobin;
use cdrs::query::*;
use config::Config;
use lazy_static::*;
use parking_lot::RwLock;
use serde::Deserialize;

mod database;

type CassandraSession = Session<RoundRobin<TcpConnectionPool<NoneAuthenticator>>>;

lazy_static! {
    static ref SETTINGS: RwLock<Config> = RwLock::new(Config::default());
}

pub enum Parameter {
    String(String),
    Int(u64),
    Float(f64),
}
pub enum Query<'a> {
    CQL(String, Option<&'a [Parameter]>),
}
trait SchemalessSource {
    fn get<T>(&self, _query: Query)
    where
        for<'de> T: Deserialize<'de>,
    {
    }
}

pub struct DataSource<T> {
    internal_source: T,
}

#[derive(Clone, Debug, TryFromRow, PartialEq)]
struct RowStruct {
    key: i16,
}

impl DataSource<CassandraSession> {
    async fn new(source: ConfigSource) -> Result<DataSource<CassandraSession>, anyhow::Error> {
        let config = read_config::<DataSourceConfig>(source)
            .await
            .map_err(|_err| anyhow!("Could not read datasource configuration"))?;

        let node_descriptions = config
            .distributed
            .expect("Can't find [distributed] in config.")
            .nodes
            .take()
            .unwrap_or_default();

        let nodes: Vec<NodeTcpConfig<NoneAuthenticator>> = node_descriptions
            .iter()
            .map(|addr| NodeTcpConfigBuilder::new(addr, NoneAuthenticator {}).build())
            .collect();

        if nodes.len() == 0 {
            return Err(anyhow!(
                "Could not create Cassandra Session, no nodes defined in [distributed]"
            ));
        }

        let cluster_config = ClusterTcpConfig(nodes);
        let session =
            new_session(&cluster_config, RoundRobin::new()).expect("session should be created");

        Ok(DataSource {
            internal_source: session,
        })
    }

    fn init(&mut self) -> &mut DataSource<CassandraSession> {
        let init_script =
            String::from_utf8_lossy(include_bytes!("../resources/init.wf")).into_owned();
        for line in init_script.lines() {
            self.internal_source.query(line).unwrap();
        }

        self
    }
    /// Setup the tablespace for agent use;
    async fn init_with(&mut self, filename: &str) -> &mut DataSource<CassandraSession> {
        let config = read_config::<DataSourceConfig>(ConfigSource::File("werkflow.toml".into()))
            .await
            .map_err(|_err| anyhow!("Could not process config file: {}", filename))
            .expect("Config file could not be found");

        let init_string: String = config
            .distributed
            .expect("[distributed] not found in config")
            .init_script
            .expect(&format!("Could not find init_script in {}", filename));

        for line in init_string.lines() {
            self.internal_source
                .query(line)
                .map_err(|err| anyhow!("Could not run query {}: {}", line, err))
                .expect("Error in script");
        }

        self
    }

    pub fn query(&mut self, query: Query) {
        match query {
            Query::CQL(q, _p) => {
                let _result = self.internal_source.query(&q);

                //result.unwrap().
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::{CassandraSession, DataSource};

    #[tokio::test(threaded_scheduler)]
    async fn test() {
        let config = r#"
    [cache]
    enabled=true

    [database]
    source_type="cassandra"
    host="localhost"
    port=9042

    [distributed]
    nodes = ["localhost:9042"]
    
    "#;

        let mut ds = DataSource::new(werkflow_config::ConfigSource::String(config.into()))
            .await
            .unwrap();

        ds.init();

        //no_compression.query(create_ks).expect("Keyspace create error");
    }
}
