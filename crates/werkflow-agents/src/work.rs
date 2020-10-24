use custom_error::custom_error;
use log::{error, trace};
use rand::Rng;
use serde::Deserialize;
use serde::Serialize;
use std::{error::Error, fmt::{self, Display}};
use tokio::{task::JoinHandle, runtime::Runtime};
use werkflow_scripting::{Dynamic, Engine, ScriptResult, to_dynamic};
use werkflow_scripting::{EvalAltResult, RegisterResultFn, Script, ScriptHost};
use anyhow::anyhow;
use crate::{comm::AgentEvent, AgentHandle, WorkloadData};

custom_error! {pub WorkloadError
    CouldNotComplete      = "Could Not Complete Error",
    Unknown            = "Unknown Error"
}

struct Resource;
enum Payload {
    Str(String),
    Int(u32),
}

#[derive(Default)]
pub struct WorkloadHandle {
    pub id: u128,
    pub status: WorkloadStatus,
    pub(crate) join_handle: Option<JoinHandle<Result<WorkloadData, WorkloadError>>>,
    pub result: Option<String>
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
            status: self.status,
            join_handle: None,
            result: None
        }
    }
}

impl WorkloadHandle {
    pub fn setup_communication() {}
    pub fn get_status() {}
}

mod http {
        use futures::executor;
use werkflow_scripting::{from_dynamic, Map, Position};

    use super::*;

    pub fn get(url: &'static str) -> Result<Dynamic, Box<EvalAltResult>>    
    {
        /// reqwest uses tokio, rhai doesn't mix with async, so I use futures to handle the tasks
        /// TODO refactor this
        
        println!("Getting {}", url);    
        let l_url = url.clone();

        let task = tokio::task::spawn_blocking(move || executor::block_on(async_get(l_url.clone())).unwrap() );
        
        if let Ok (d) = executor::block_on(task) {
            return Ok(d)
        } else {
            Err(Box::new(EvalAltResult::ErrorRuntime(
                format!("Could not deserialize result"),
                Position::none(),
            )))
        }        
    }

    pub fn post(url: &'static str, body: Dynamic) -> Result<Dynamic, Box<EvalAltResult>>        
    {
        
        let l_url = url.clone();
        
        let task = tokio::task::spawn_blocking(move || executor::block_on(async_post(l_url, body)).unwrap() );

        match  executor::block_on(task) {
            Ok ( r) => {
                Ok(r)
            },
            Err(err) => {
                Err(Box::new(EvalAltResult::ErrorRuntime(
                    format!("Could not run post, general error: {}, {}", err, url),
                    Position::none(),
                )))
            }
        }
    }

    pub async fn async_get(url: &str) -> Result<Dynamic, Box<dyn Error>>
    {
        let response = reqwest::Client::new()
            .get(url)
            .header("User-Agent", "werkflow-agent/0.1.0")
            .send()
            .await?
            .text()
            .await?;
            
        Ok(to_dynamic(response).unwrap())
        /*
        let t = serde_json::from_str::<T>(&response)?;

        Ok(to_dynamic(t)?)*/
    }

    /// Convert a dynamic to a JSON string, use it as the body of a post request, and then respond with the
    /// same type
    /// This allows scripts to use maps of any time to call this:
    /// let user = post(url, #{ fieldOnT: someSetting })
    pub async fn async_post(url: &str, body: Dynamic) -> Result<Dynamic, Box<dyn Error>>
    {
        let body = serde_json::to_string(&from_dynamic(&body)?)?;
        let response = reqwest::Client::new()
            .post(url)
            .body(body)
            .header("User-Agent", "werkflow-agent/0.1.0")
            .send()
            .await?
            .text()
            .await?;

        Ok(to_dynamic(response).unwrap())
        //Ok(to_dynamic(response)?)
    }
}

#[derive(Clone, Debug)]
pub struct Workload {
    pub id: u128,
    pub script: Script,
    agent_handle: AgentHandle
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum WorkloadStatus {
    Running,
    Error(&'static str),
    Complete,
    None,
}

impl Display for WorkloadStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            WorkloadStatus::Running => {             
                f.write_str("Running")
            }
            WorkloadStatus::Error(s) => {                
                f.write_fmt(format_args!("Error: {}", s))
            }
            WorkloadStatus::Complete => {        
                f.write_str("Complete")}
            WorkloadStatus::None => {                        
                f.write_str("None")
            }
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
        self.to::<WorkloadData>()
    }
}

impl Workload {
    pub fn new(agent_handle : AgentHandle) -> Workload {
        Workload {
            id: rand::thread_rng().gen(),
            script: Script::default(),
            agent_handle
        }
    }
    pub fn with_script(agent_handle: AgentHandle, script: Script) -> Workload {
        Workload {
            id: rand::thread_rng().gen(),
            script,
            agent_handle 
        }
    }
    
    pub async fn run(&self) -> Result<WorkloadData, WorkloadError>        
    {
        let mut sh = ScriptHost::new();

        {
            let agent = self.agent_handle.handle.read().await;
            
            let channel = agent.hub.write().await.get_or_create("work");

            let _ = channel.sender.send(AgentEvent::WorkStarted(self.clone()))
                .map_err(|err| anyhow!("{}", err));
        }
        // Would like to refactor this so types can be infered from use, maybe a proc macro
        // to create all of the funcs

        sh.engine.register_result_fn("get", http::get);
        sh.engine.register_result_fn("post", http::post);

        trace!("Executing script");
        match sh.execute(self.script.clone()).await {
            Ok(result) => {                
                trace!("Done executing script");
                Ok(result.into())
            }
            _ => {
                error!("Error running script");
                Err(WorkloadError::Unknown)
            },
        }

    }
}

#[cfg(test)]
mod test {
            use futures::executor;

use crate::Agent;

use super::*;
    #[derive(Serialize, Deserialize, PartialEq, Default)]
    #[serde(default)]
    struct User {
        id: u32,
        name: String,
    }

    #[test]
    fn test_stuff() {
        let runtime = Runtime::new().unwrap();
        let agent = AgentHandle::with_runtime("Test Agent", runtime);

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
        let workload = Workload::with_script(agent.clone(),script);
        let workload2 = Workload::with_script(agent.clone(),script2);

        let user = serde_json::from_str::<User>(&executor::block_on(workload.run()).unwrap().result).unwrap();
        let user2 = serde_json::from_str::<User>(&executor::block_on(workload2.run()).unwrap().result).unwrap();

        assert_eq!(11, user.id);
        assert_eq!(1, user2.id);
        assert_eq!(user2.name, "Leanne Graham".to_string());
    }
}
