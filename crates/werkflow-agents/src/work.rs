use crate::cfg::AgentConfiguration;
use crate::AsyncRunner;
use werkflow_config::read_config;
use werkflow_config::ConfigSource;
use werkflow_core::sec::ZoneRecord;
use werkflow_core::{
    sec::{DnsProvider, Zone},
    HttpAction,
};
use werkflow_scripting::{Array, ImmutableString};

use log::{error, trace};
use rand::Rng;
use serde::ser::{Serialize, SerializeMap, SerializeSeq, SerializeStruct, Serializer};
use std::collections::HashMap;
use werkflow_scripting::Map;

use crate::{comm::AgentEvent, AgentController, WorkloadData};
use anyhow::{anyhow, Result};
use std::fmt::{self, Display};
use tokio::task::JoinHandle;
use werkflow_scripting::{to_dynamic, Dynamic, ScriptResult};
use werkflow_scripting::{EvalAltResult, RegisterFn, RegisterResultFn, Script, ScriptHost};

#[derive(Default)]
pub struct WorkloadHandle {
    pub id: u128,
    pub status: WorkloadStatus,
    pub join_handle: Option<JoinHandle<Result<String>>>,
    pub workload: Option<Workload>,
    pub result: Option<String>,
}

impl PartialEq for WorkloadHandle {
    fn eq(&self, other: &Self) -> bool {
        other.id == self.id
    }
}
impl Clone for WorkloadHandle {
    fn clone(&self) -> Self {
        return WorkloadHandle {
            id: self.id,
            status: self.status.clone(),
            join_handle: None,
            result: None,
            workload: self.workload.clone()
        };
    }
}

impl WorkloadHandle {
    pub fn setup_communication() {}
    pub fn get_status() {}
}

mod http {
    use crate::threads::AsyncRunner;

    use super::*;

    pub fn get(url: &'static str) -> Result<Dynamic, Box<EvalAltResult>> {
        println!("Getting {}", url);
        let l_url = url.clone();

        Ok(AsyncRunner::block_on(async_get(l_url.clone())))
    }

    pub fn post(url: &'static str, body: Map) -> Result<Dynamic, Box<EvalAltResult>> {
        let l_url = url.clone();

        Ok(AsyncRunner::block_on(async_post(
            l_url.clone(),
            SerMap { underlying: body },
        )))
    }

    pub async fn async_get(url: &str) -> Dynamic {
        let response = reqwest::Client::new()
            .get(url)
            .header("User-Agent", "werkflow-agent/0.1.0")
            .send()
            .await
            .map_err(|err| anyhow!("Error contacting url {}, {}", url, err))
            .unwrap()
            .text()
            .await
            .map_err(|err| anyhow!("Could not make the result text, {}", err))
            .unwrap();

        to_dynamic(response).unwrap()
    }

    /// Convert a dynamic to a JSON string, use it as the body of a post request, and then respond with the
    /// same type
    /// This allows scripts to use maps of any time to call this:
    /// let user = post(url, #{ fieldOnT: someSetting })
    pub async fn async_post(url: &str, body: SerMap) -> Dynamic {
        let body = serde_json::to_string(&body)
            .map_err(|_err| anyhow!("Error contacting url {}", url))
            .unwrap();

        let response = reqwest::Client::new()
            .post(url)
            .body(body)
            .header("User-Agent", "werkflow-agent/0.1.0")
            .send()
            .await
            .map_err(|_err| anyhow!("Error contacting url {}", url))
            .unwrap()
            .text()
            .await
            .map_err(|_err| anyhow!("Could not make the result text"))
            .unwrap();

        to_dynamic(response)
            .map_err(|_err| anyhow!("Could not make result from script dynamic"))
            .unwrap()
        //Ok(to_dynamic(response)?)
    }
}

#[derive(Clone, Debug)]
pub struct Workload {
    pub id: u128,
    pub script: Script,
    agent_handle: AgentController,
}

#[derive(Clone, Debug, PartialEq)]
pub enum WorkloadStatus {
    Running,
    Error(String),
    Complete,
    None,
}

impl Display for WorkloadStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            WorkloadStatus::Running => f.write_str("Running"),
            WorkloadStatus::Error(s) => f.write_fmt(format_args!("Error: {}", s)),
            WorkloadStatus::Complete => f.write_str("Complete"),
            WorkloadStatus::None => f.write_str("None"),
        }
    }
}
impl Default for WorkloadStatus {
    fn default() -> Self {
        WorkloadStatus::None
    }
}

impl Into<WorkloadData> for ScriptResult {
    fn into(self) -> WorkloadData {
        self.to::<WorkloadData>().unwrap()
    }
}

pub struct SerMap {
    underlying: Map,
}

enum SerializableField {
    Str(String),
    Int(i64),
    Float(f64),
    Bool(bool),
    Array(Vec<SerializableField>),
    Map(HashMap<String, SerializableField>),
    None,
}

impl SerializableField {
    fn from_dynamic(dynamic: Dynamic) -> SerializableField {
        match &dynamic.type_name()[..] {
            "map" => {
                let mut hash_map = HashMap::new();

                for (k, v) in dynamic.try_cast::<Map>().unwrap() {
                    let f = SerializableField::from_dynamic(v);
                    hash_map.insert(k.to_string(), f);
                }

                SerializableField::Map(hash_map)
            }
            "i64" => SerializableField::Int(dynamic.try_cast::<i64>().unwrap()),
            "f64" => SerializableField::Float(dynamic.try_cast::<f64>().unwrap()),
            "bool" => SerializableField::Bool(dynamic.try_cast::<bool>().unwrap()),
            "string" => SerializableField::Str(dynamic.try_cast::<String>().unwrap()),
            "array" => {
                let vec = dynamic.try_cast::<Vec<Dynamic>>().unwrap().clone();
                let mut ovec = Vec::new();

                for v in vec {
                    let f = SerializableField::from_dynamic(v);
                    ovec.push(f);
                }

                SerializableField::Array(ovec)
            }
            _ => SerializableField::None,
        }
    }
}

impl Serialize for SerializableField {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        //let mut state = serializer.serialize_struct("Field", self.underlying.len())?;
        match self {
            SerializableField::Str(v) => serializer.serialize_str(&v.clone()),
            SerializableField::Int(v) => serializer.serialize_i64(*v),
            SerializableField::Float(v) => serializer.serialize_f64(*v),
            SerializableField::Bool(v) => serializer.serialize_bool(*v),
            SerializableField::Map(v) => {
                let mut map = serializer.serialize_map(Some(v.len()))?;
                for (k, v) in v {
                    map.serialize_entry(k, v)?;
                }
                map.end()
            }
            SerializableField::None => serializer.serialize_none(),
            SerializableField::Array(s) => {
                let mut seq = serializer.serialize_seq(Some(s.len()))?;
                for e in s.clone() {
                    seq.serialize_element(e)?;
                }
                seq.end()
            }
        }
    }
}

impl Serialize for SerMap {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let mut state = serializer.serialize_struct("SerMap", self.underlying.len())?;
        let underlying = self.underlying.clone();

        for (key, v) in underlying {
            let k: &str = Box::leak(key.to_string().into_boxed_str());

            match SerializableField::from_dynamic(v) {
                SerializableField::Str(v) => state.serialize_field(&k, &v)?,
                SerializableField::Int(v) => state.serialize_field(&k, &v)?,
                SerializableField::Float(v) => state.serialize_field(&k, &v)?,
                SerializableField::Bool(v) => state.serialize_field(&k, &v)?,
                SerializableField::Map(v) => state.serialize_field(&k, &v)?,
                SerializableField::None => {}
                SerializableField::Array(v) => state.serialize_field(&k, &v)?,
            }
        }

        state.end()
    }
}

#[derive(Clone)]
pub struct CommandHost {
    config: AgentConfiguration,
}

impl CommandHost {
    fn new_ch(filename: ImmutableString) -> CommandHost {
        let config =
            AsyncRunner::block_on(
                async move { read_config(ConfigSource::File(filename.into())).await },
            );
        CommandHost {
            config: config.unwrap(),
        }
    }
    fn new_ch_web(url: ImmutableString) -> CommandHost {
        let config = AsyncRunner::block_on(async move {
            read_config(ConfigSource::Http(HttpAction::Get(url.into()))).await
        });
        CommandHost {
            config: config.unwrap(),
        }
    }
    fn create_container(&mut self, 
        name : ImmutableString, 
        image : ImmutableString, 
        env : Array) {
        
        let s =  AsyncRunner::block_on(werkflow_container::ContainerService::default_connect());
        let environment : Vec<String> = env.iter().map(|s| s.to_string()).collect::<Vec<String>>().clone();

        AsyncRunner::block_on(async move { 
            s.create_and_start_container(name.to_string(),image.to_string(), environment.as_slice())
                .await
                .unwrap();
        });
    }
    
    /// Start a container from an image name.
    /// Env is a list of strings VAR=WHATEVER
    /// port_forward is a map of strings such that the rhai map literal #{ "3000/tcp": "3000"  } would map local port 3000 to 3000/tcp
    fn start_container(&mut self, 
        name : ImmutableString, 
        image : ImmutableString, 
        env : Array,
        ports: Map) {
        
        let s =  AsyncRunner::block_on(werkflow_container::ContainerService::default_connect());
        // convert envs and forwards.
        let environment : Vec<String> = env.iter().map(|s| s.to_string()).collect::<Vec<String>>().clone();
        let mut forwards : HashMap<String, String> = HashMap::new();
        
        ports.iter().for_each(|(k, v)| {
            forwards.insert(k.to_string(), v.to_string());
        });

        AsyncRunner::block_on(async move { 
            s.start_container(name.to_string(),image.to_string(), environment.as_slice(), forwards.clone())
                .await
                .unwrap();
        });
    }
    // add a DNS A record
    fn add_a_record(
        &mut self,
        zone: String,
        host: String,
        target: String
    ) {
        if let Some(dns_cfg) = &self.config.dns {
            let provider = DnsProvider::new(&dns_cfg.api_key).unwrap();

            let result =
                AsyncRunner::block_on(async move { provider.add_or_replace(&Zone::ByName(zone), &ZoneRecord::A(host, target)).await });

            match result {
                Ok(_) => print!("Zone added"),
                Err(err) => println!("Error adding zone: {}", err),
            };
        } else {
            println!("No DNS Configuration");
        }
    }
}

impl Workload {
    pub fn new(agent_handle: AgentController) -> Workload {
        Workload {
            id: rand::thread_rng().gen(),
            script: Script::default(),
            agent_handle,
        }
    }
    pub fn with_script(agent_handle: AgentController, script: Script) -> Workload {
        Workload {
            id: rand::thread_rng().gen(),
            script,
            agent_handle,
        }
    }

    pub async fn run(&self) -> Result<String> {
        let mut script_host = ScriptHost::new();

        let _ = self
            .agent_handle
            .send(AgentEvent::WorkStarted(self.clone()))
            .unwrap();

        // register command host functions, this will likely be refactored into different
        // modules e.g. Web, DNS, Container, Cloud
        script_host.with_engine(|e| {
            e.register_type::<CommandHost>();
            e.register_fn("config", CommandHost::new_ch);
            e.register_fn("config_web", CommandHost::new_ch_web);
            e.register_fn("add_record", CommandHost::add_a_record);
            e.register_fn("start_container", CommandHost::start_container);
            e.register_fn("create_container", CommandHost::create_container);
            e.register_result_fn("get", http::get);
            e.register_result_fn("post", http::post);
        });

        trace!("Executing script");

        match script_host.execute(self.script.clone()).await {
            Ok(result) => {
                println!("Done executing script {:?}", result);
                Ok(result.underlying.to_string())
            }
            Err(err) => {
                error!("Error running script");
                Err(anyhow!("Unknown error executing script {}", err))
            }
        }
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use serde::*;
    
    use tokio::runtime::Runtime;
    use werkflow_scripting::Engine;

    #[derive(Serialize, Deserialize, PartialEq, Default)]
    #[serde(default)]
    struct User {
        id: u32,
        name: String,
    }

    #[test]
    fn test_ser() {
        let engine = Engine::new();

        let result: Dynamic = engine
            .eval(
                r#"
            #{
                a: 42,
                b: [ "hello", "world" ],
                c: true,
                d: #{ x: 123.456, y: 999.0 }
            }
        "#,
            )
            .unwrap();

        println!(
            "{}",
            serde_json::to_string_pretty(&SerMap {
                underlying: result.try_cast::<Map>().unwrap()
            })
            .unwrap()
        )
    }

    /// ```
    /// // more complex workload here.
    ///     let r = Runtime::new().unwrap();
    ///     let handle = r.handle().clone();
    ///     let agent = AgentController::with_runtime("test-agent".into(), r);
    /// 
    ///     let script = Script::new(
    ///     r#"                
    ///     let ch = config("../../config/werkflow.toml");
    /// 
    ///              ch.add_record("autobuild.cloud", "test-script", "127.0.0.1");
    /// 
    ///              ch.start_container("test-g", "grafana/grafana", [], #{ "3000/tcp": "3000"  });
    ///           "#,
    ///        );
    ///        let workload = Workload::with_script(agent.clone(), script);
    ///  
    ///       handle.block_on(async move {
    ///           &workload.run().await.unwrap();
    ///      });
    /// ```
    #[tokio::test(threaded_scheduler)]
    async fn test_stuff() {
        let agent = AgentController::new("Test Agent".into());

        let script = Script::new(
            r#"                
                let user = post("https://jsonplaceholder.typicode.com/users", #{ name: "Test".to_string() } );
                print("user: " + user);
                user
            "#,
        );
        let script2 = Script::new(
            r#"                
                let user = get("https://jsonplaceholder.typicode.com/users/1" );
                print("user: " + user);
                user
            "#,
        );
        let workload = Workload::with_script(agent.clone(), script);
        let workload2 = Workload::with_script(agent.clone(), script2);

        let user = serde_json::from_str::<User>(&workload.run().await.unwrap()).unwrap();
        let user2 = serde_json::from_str::<User>(&workload2.run().await.unwrap()).unwrap();

        assert_eq!(11, user.id);
        assert_eq!(1, user2.id);
        assert_eq!(user2.name, "Leanne Graham".to_string());
    }

}
