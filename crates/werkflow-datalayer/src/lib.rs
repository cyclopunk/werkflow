#![feature(type_alias_impl_trait)]

use cdrs::{cluster::TcpConnectionPool, query_values};
use database::DataSourceConfig;
use werkflow_config::{read_config, ConfigSource};

use cdrs::types::prelude::{TryFromRow, TryFromUDT};
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

pub enum Query<T> where T : Into<QueryValues> {
    CQL(String, Option<T>),
}
trait SchemalessSource {
    fn get<T : Into<QueryValues> >(&self, _query: Query<T>)
    where
        for<'de> T: Deserialize<'de>,
    {
    }
}

pub struct DataSource<T> {
    internal_source: T,
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

    pub fn query<T : TryFromUDT + std::fmt::Debug + TryFromRow, F: Into<QueryValues>>(&mut self, query: Query<F>) -> Vec<T> {
        let mut vec_of_ts: Vec<T> = Vec::new();

        match query {
            Query::CQL(q, t) => {

                let rows = match t {
                    Some(qv) => {
                        self.internal_source
                            .query_with_values(q, qv)
                    }
                    None => {
                        self.internal_source
                            .query(q)
                    }
                }.expect("query")
                    .get_body()
                    .expect("get body")
                    .into_rows();
                if let Some(rows) = rows {
                    for row in rows {
                        let my_row: T = T::try_from_row(row).expect("could not turn into user");
                        vec_of_ts.push(my_row);
                    }
                }
                //result.unwrap().
            }
        }

        return vec_of_ts;
    }
}

macro_rules! map(
    { $($key:expr => $value:expr),+ } => {
        {
            let mut m = ::std::collections::HashMap::new();
            $(
                m.insert($key, $value);
            )+
            m
        }
     };
);

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
use super::*;
    
    use cdrs::types::prelude::*;
    use cdrs::frame::IntoBytes;
    use cdrs::types::from_cdrs::FromCDRSByName;
    #[derive(Clone, Debug, TryFromRow, TryFromUDT, IntoCDRSValue, PartialEq, Default)]
    struct User {
        id : i64,
        username : String
    }
    
    impl Into<QueryValues> for User {
        
        fn into(self) -> QueryValues { 
            query_values!("username" => self.username, "id" => self.id)
         }
    }    

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
        let resp : Vec<User> = ds.query(Query::CQL("insert into user (id, username) values (?,?); ".into(), Some(User { username: "adam".to_string(), id: 1} )));
        let users: Vec<User> = ds.query::<User,HashMap<&str,&str>>(Query::CQL("select * from user where username = ? ALLOW FILTERING;".into(), 
            Some(map! {"username" => "adam" })));

        assert_eq!(users.len(), 1);

        //no_compression.query(create_ks).expect("Keyspace create error");
    }
}
