use custom_error::custom_error;
use rand::Rng;
use serde::Deserialize;
use serde::Serialize;
use std::{error::Error, fmt::{self, Display}};
use tokio::{task::JoinHandle, runtime::Runtime};
use werkflow_scripting::{to_dynamic, Dynamic, Engine};
use werkflow_scripting::{EvalAltResult, RegisterResultFn, Script, ScriptHost};

use crate::{AgentHandle, WorkloadData};

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
    pub(crate) join_handle: Option<JoinHandle<Result<WorkloadData, WorkloadError>>>
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
            join_handle: None
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

    pub fn get<T>(url: &str) -> Result<Dynamic, Box<EvalAltResult>>
    where
        for<'a> T: Serialize + Deserialize<'a>,
    {
        /// reqwest uses tokio, rhai doesn't mix with async, so I use futures to handle the tasks
        /// TODO refactor this
        let response = executor::block_on(reqwest::Client::new()
            .get(url)
            .header("User-Agent", "werkflow-agent/0.1.0")
            .send());
        match response {
            Ok ( r) => {
                let text =  executor::block_on(r.text()).unwrap_or_default();

                if let Ok(obj) = serde_json::from_str::<T>(&text){
                    Ok(to_dynamic(obj).unwrap())
                } else {
                    Err(Box::new(EvalAltResult::ErrorRuntime(
                        format!("Could not deserialize result"),
                        Position::none(),
                    )))
                }
            },
            Err(err) => {
                Err(Box::new(EvalAltResult::ErrorRuntime(
                    format!("Could not run post, general error: {}", err),
                    Position::none(),
                )))
            }
        }
    }

    pub fn post<T>(url: &str, body: Map) -> Result<Dynamic, Box<EvalAltResult>>
    where
        for<'a> T: Serialize + Deserialize<'a>,
    {
        
        let body = serde_json::to_string(&from_dynamic::<T>(&body.into()).unwrap()).unwrap();

        let response = reqwest::Client::new()
            .post(url)
            .body(body)
            .header("User-Agent", "werkflow-agent/0.1.0")
            .send();
        match  executor::block_on(response) {
            Ok ( r) => {
                let text =  executor::block_on(r.text()).unwrap_or_default();

                if let Ok(obj) = serde_json::from_str::<T>(&text){
                    Ok(to_dynamic(obj).unwrap())
                } else {
                    Err(Box::new(EvalAltResult::ErrorRuntime(
                        format!("Could not deserialize result"),
                        Position::none(),
                    )))
                }
            },
            Err(err) => {
                Err(Box::new(EvalAltResult::ErrorRuntime(
                    format!("Could not run post, general error: {}", err),
                    Position::none(),
                )))
            }
        }
    }

    pub async fn async_get<T>(url: &str) -> Result<Dynamic, Box<dyn Error>>
    where
        for<'a> T: Serialize + Deserialize<'a>,
    {
        let response = reqwest::Client::new()
            .get(url)
            .header("User-Agent", "werkflow-agent/0.1.0")
            .send()
            .await?
            .text()
            .await?;

        let t = serde_json::from_str::<T>(&response)?;

        Ok(to_dynamic(t)?)
    }

    /// Convert a dynamic to a JSON string, use it as the body of a post request, and then respond with the
    /// same type
    /// This allows scripts to use maps of any time to call this:
    /// let user = post(url, #{ fieldOnT: someSetting })
    pub async fn async_post<'de, T>(url: &str, body: Dynamic) -> Result<Dynamic, Box<dyn Error>>
    where
        for<'a> T: Serialize + Deserialize<'a>,
    {
        let body = serde_json::to_string(&from_dynamic::<T>(&body)?)?;
        let response = reqwest::Client::new()
            .post(url)
            .body(body)
            .header("User-Agent", "werkflow-agent/0.1.0")
            .send()
            .await?
            .text()
            .await?;

        Ok(to_dynamic(response)?)
    }
}

#[derive(Clone, Debug)]
pub struct Workload {
    pub id: u128,
    script: Script,
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
    pub async fn run<T>(&self) -> Result<T, WorkloadError>
    where
        for<'de> T: Serialize + Deserialize<'de> + 'static,
    {
        let mut sh = ScriptHost::new();

        // Would like to refactor this so types can be infered from use, maybe a proc macro
        // to create all of the funcs

        sh.engine.register_result_fn("get", http::get::<T>);
        sh.engine.register_result_fn("post", http::post::<T>);

        match sh.execute(self.script.clone()).await {
            Ok(result) => Ok(result.to()),
            _ => Err(WorkloadError::Unknown),
        }
    }
}

#[cfg(test)]
mod test {
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
        let mut runtime = Runtime::new().unwrap();
        let mut agent = AgentHandle::with_runtime("Test Agent", runtime);

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
        let workload = Workload::with_script(agent,script);
        let workload2 = Workload::with_script(agent,script2);

        let user = runtime.block_on(workload.run::<User>()).unwrap();
        let user2 = runtime.block_on(workload2.run::<User>()).unwrap();

        assert_eq!(11, user.id);
        assert_eq!(1, user2.id);
        assert_eq!(user2.name, "Leanne Graham".to_string());
    }
}
