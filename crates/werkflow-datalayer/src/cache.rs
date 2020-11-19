
use std::{collections::HashMap, rc::Rc, sync::{Arc, RwLock}};

use werkflow_scripting::{Map, RegisterFn, ScriptEngine, ScriptEnginePlugin};
use redis::{Commands, Connection, RedisResult};
use lazy_static::lazy_static;

lazy_static! {
    pub static ref CONNECTIONS: RwLock<HashMap<String, Arc<RwLock<Connection>>>> = RwLock::new(HashMap::new());
}

pub struct RemoteStorage {
    connection_string: String,
    connection: Arc<RwLock<Connection>>
}

impl Clone for RemoteStorage {
    fn clone(&self) -> Self {
        println!("Clone called");
        RemoteStorage {
            connection_string: self.connection_string.clone(),
            connection: self.connection.clone()
        }
    }
}
pub struct RemoteStoragePlugin;

impl ScriptEnginePlugin for RemoteStoragePlugin {
    fn init(&self, host: &mut ScriptEngine) {
        host.engine
            .register_type::<RemoteStorage>()
            .register_fn("remote_storage", RemoteStorage::new)
            .register_fn("set", RemoteStorage::set_int)
            .register_fn("set", RemoteStorage::set_str)
            .register_fn("set", RemoteStorage::set_float)
            .register_fn("get", RemoteStorage::get_int)
            .register_fn("get", RemoteStorage::get_str)
            .register_fn("get", RemoteStorage::get_float);

    }
}
impl RemoteStorage {
   pub fn new (connection_string: &str ) -> RemoteStorage {
       let mut conns = CONNECTIONS.write().unwrap();
       let conn = match  conns.get(connection_string) {
           Some(c) => {
               Arc::clone(c)
           }
           None => {
               let c = Arc::new(RwLock::new(connect(connection_string).unwrap()));
               conns.insert(connection_string.clone().to_string(), c.clone());
               c
           }
       };
     
       RemoteStorage { 
        connection_string: connection_string.to_string(), 
        connection: conn
       }
   } 
   pub fn get_str(&mut self, key : &str) -> String {
        self.connection.write().unwrap().get::<&str, String>(key).unwrap()
   }
   pub fn get_float(&mut self, key : &str) -> f64 {
        self.connection.write().unwrap().get::<&str, f64>(key).unwrap_or_default()
    }
    pub fn get_int(&mut self, key : &str) -> i64 {
        self.connection.write().unwrap().get::<&str, i64>(key).unwrap_or_default()
    }
   pub fn set_str(&mut self, key : &str, value: &str) -> String {
        self.connection.write().unwrap().set::<&str, &str,String>(key, value).unwrap()
   }
   pub fn set_int(&mut self, key : &str, value: i64) -> i64 {
        self.connection.write().unwrap().set::<&str, i64, i64>(key, value).unwrap()
   }
   pub fn set_float(&mut self, key : &str, value: f64) -> f64 {
        self.connection.write().unwrap().set::<&str, f64, f64>(key, value).unwrap_or_default()
   }
}

pub fn connect(connection_string: &str) -> RedisResult<Connection> {
    let client = redis::Client::open(connection_string)?;

    let con = client.get_connection()?;
    /* do something here */
    
    Ok(con)
}

#[cfg(test)]
mod tests {
    use crate::database::CacheConfig;
    use werkflow_config::read_config;

    use super::*;
    use redis::Commands;
    #[tokio::test(threaded_scheduler)]
    async fn test_read_config() -> Result<(), anyhow::Error> {
        let config_file = r#"        
        enabled=true
        host="redis://127.0.0.1:9042/"
        "#;

        let cfg: CacheConfig =
            read_config(werkflow_config::ConfigSource::String(config_file.into())).await?;

        let mut x = connect(&cfg.host)?;

        x.set("test", 42)?;

        assert_eq!(x.get::<&str, i8>("test").unwrap(), 42);

        Ok(())
    }
}
