#![feature(type_alias_impl_trait)]

use cdrs::frame::IntoBytes;
use cdrs::{cluster::TcpConnectionPool};
use database::DataSourceConfig;
use werkflow_config::{read_config, ConfigSource};

use cdrs::types::prelude::{TryFromRow, TryFromUDT};
use cdrs_helpers_derive::*;

use cdrs::types::from_cdrs::FromCDRSByName;

use anyhow::{anyhow, Result};
use cdrs::authenticators::NoneAuthenticator;
use cdrs::cluster::session::new as new_session;
use cdrs::cluster::{session::Session, NodeTcpConfig};
use cdrs::cluster::{ClusterTcpConfig, NodeTcpConfigBuilder};

use cdrs::load_balancing::RoundRobin;
use cdrs::query::*;

mod cache;
mod database;

#[path = "events/amqp.rs"]
mod amqp;

type CassandraSession = Session<RoundRobin<TcpConnectionPool<NoneAuthenticator>>>;
pub enum Query<T>
where
    T: Into<QueryValues>,
{
    Raw(String),
    RawWithValues(String, T),
}

pub struct DataSource<T> {
    source: T,
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

        Ok(DataSource { source: session })
    }

    fn init(&mut self) -> &mut DataSource<CassandraSession> {
        let init_script =
            String::from_utf8_lossy(include_bytes!("../resources/init.wf")).into_owned();
        for line in init_script.lines() {
            self.source.query(line).unwrap();
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
            self.source
                .query(line)
                .map_err(|err| anyhow!("Could not run query {}: {}", line, err))
                .expect("Error in script");
        }

        self
    }

    pub fn query<T: TryFromUDT + std::fmt::Debug + TryFromRow, F: Into<QueryValues>>(
        &mut self,
        query: Query<F>,
    ) -> Result<Vec<T>> {
        let mut vec_of_ts: Vec<T> = Vec::new();

        let result = match query {
            Query::RawWithValues::<F>(q, t) => self
                .source
                .query_with_values(q, t)
                .map_err(|err| anyhow!("Could not run query: {}", err))?,
            Query::Raw(q) => self
                .source
                .query(q)
                .map_err(|err| anyhow!("Could not run query: {}", err))?,
        };

        let rows = result
            .get_body()
            .map_err(|err| anyhow!("Could not get body: {}", err))?
            .into_rows();

        if let Some(rows) = rows {
            for row in rows {
                let my_row: T = T::try_from_row(row).expect("could not turn into user");
                vec_of_ts.push(my_row);
            }
        }

        return Ok(vec_of_ts);
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

#[derive(Clone, Debug, TryFromRow, TryFromUDT, IntoCDRSValue, PartialEq, Default)]
struct NoValue {
    none: i64,
}

#[cfg(test)]
mod tests {

    use super::*;

    use cdrs::{frame::IntoBytes, query_values};
    use cdrs::types::from_cdrs::FromCDRSByName;
    #[derive(Clone, Debug, TryFromRow, TryFromUDT, IntoCDRSValue, PartialEq, Default)]
    struct User {
        id: i64,
        username: String,
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

        let mut ds: DataSource<CassandraSession> =
            DataSource::new(werkflow_config::ConfigSource::String(config.into()))
                .await
                .unwrap();

        ds.init();

        for i in 1..50 {
            let _: Vec<User> = ds
                .query(Query::RawWithValues(
                    "insert into user (id, username) values (?,?); ".into(),
                    User {
                        username: "adam".into(),
                        id: i,
                    },
                ))
                .unwrap();
        }

        println!("Inserted 50 users");

        let users: Vec<User> = ds
            .query(Query::RawWithValues(
                "select * from user where id = ?".into(),
                map! {"id" => 42 },
            ))
            .unwrap();

        assert_eq!(users.len(), 1);

        ds.query::<NoValue, User>(Query::Raw("truncate user; ".into()))
            .unwrap();
    }
}
