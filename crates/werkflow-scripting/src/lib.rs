use log::{debug, info};
use rand::prelude::*;
use rhai::Scope;
use serde_json::Value;
use state::HostState;
use std::{collections::HashMap, fmt};

pub use rhai::serde::*;

pub use rhai::{
    Array, Dynamic, Engine, EvalAltResult, ImmutableString, Map, Position, RegisterFn,
    RegisterResultFn,
};

use serde::{Deserialize, Serialize};

mod functions;
pub mod state;
pub mod scheduler;

#[derive(Debug, PartialEq, Clone, Serialize, Deserialize, Default)]
pub struct ScriptIdentifier {
    pub id: u128,
    pub name: String,
}

impl Into<ScriptIdentifier> for &str {
    fn into(self) -> ScriptIdentifier {
        let mut rng = rand::thread_rng();

        ScriptIdentifier {
            id: rng.gen(),
            name: self.to_string(),
        }
    }
}
#[derive(Debug, PartialEq, Clone, Serialize, Deserialize, Default)]
pub struct Script {
    pub identifier: ScriptIdentifier,
    pub body: String,
}

impl Script {
    pub fn new(script_body: &str) -> Script {
        return Script {
            identifier: "Unnamed Script".into(),
            body: script_body.to_string(),
        };
    }
    pub fn with_name<T: Into<ScriptIdentifier>>(id: T, script_body: &str) -> Script {
        return Script {
            identifier: id.into(),
            body: script_body.to_string(),
        };
    }
}
/// Wrapper object for the underlying scripting engine.

pub struct ScriptEngine<'a> {
    pub engine: Engine,
    pub scope: Scope<'a>,
}

#[derive(PartialEq, Clone, Serialize, Deserialize, Default)]
pub struct ScriptEngineError {
    file: String,
    line: usize,
    error_text: String,
}

// Implement std::fmt::Display for AppError
impl fmt::Display for ScriptEngineError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "An Error Occurred, Please Try Again!") // user-facing output
    }
}

// Implement std::fmt::Debug for AppError
impl fmt::Debug for ScriptEngineError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(
            f,
            "{{ file: {}, line: {} error: {} }}",
            self.file, self.line, self.error_text
        ) // programmer-facing output
    }
}

#[derive(Debug, Clone)]
pub struct ScriptResult {
    is_error: bool,
    pub underlying: Value,
}

impl ScriptResult {
    pub fn to<T>(&self) -> Result<T, ScriptEngineError>
    where
        for<'de> T: Deserialize<'de>,
    {
        let bare_str = self.underlying.as_str().unwrap_or_default();
        serde_json::from_value::<T>(self.underlying.clone()).map_err(|_err| ScriptEngineError {
            error_text: format!("Could not deserialize {} into struct", bare_str).into(),
            ..Default::default()
        })
    }
}

pub trait ScriptEnginePlugin {
    fn init(&self, host: &mut ScriptEngine);
}

impl Default for ScriptEngine<'_> {
    fn default() -> Self {
        ScriptEngine {
            engine: Engine::new(),
            scope: Scope::new(),
        }
    }
}
impl<'a> ScriptEngine<'a> {
    pub fn with_default_plugins() -> ScriptEngine<'a> {
        let mut host = ScriptEngine::default();

        host.add_plugin(HostState::new())
            .add_plugin(functions::rand::Plugin);

        host
    }

    pub fn with_engine<T>(&mut self, func: T)
    where
        for<'fo> T: FnOnce(&'fo mut Engine),
    {
        func(&mut self.engine);
    }

    pub fn add_plugin<S: ScriptEnginePlugin>(&mut self, plugin: S) -> &mut ScriptEngine<'a> {
        plugin.init(self);

        self
    }

    pub fn execute(&mut self, script: Script) -> Result<ScriptResult, ScriptEngineError> {
        debug!("Start running script in Script Host:\n {:?}", script);
        let mut scope = self.scope.clone();

        let result = self
            .engine
            .eval_with_scope::<Dynamic>(&mut scope, &script.body)
            .map_err(|err| ScriptEngineError {
                error_text: err.to_string(),
                line: err.position().position().unwrap_or_default(),
                file: script.identifier.name.clone(),
            });

        // update the scope after running the script

        self.scope = scope;

        info!(
            "Done running script {} in Script Host",
            script.identifier.name
        );

        match result {
            Ok(r) => {
                let bare_str = r.as_str().unwrap_or_default();

                let val: Value = if r.is::<String>() {
                    if let Ok(obj) = serde_json::from_str(bare_str) {
                        obj
                    } else {
                        // support for returned raw strings
                        serde_json::from_str(&format!("\"{}\"", bare_str)).unwrap()
                    }
                } else {
                    from_dynamic::<Value>(&r).map_err(|_err| ScriptEngineError {
                        error_text: format!("Could not deserialize {} into struct", bare_str)
                            .into(),
                        ..Default::default()
                    })?
                };
                Ok(ScriptResult {
                    is_error: false,
                    underlying: serde_json::to_value(val).unwrap(),
                })
            }
            Err(err) => {
                println!("Error running script {:?}", err);
                Ok(ScriptResult {
                    is_error: true,
                    underlying: serde_json::to_value(err.to_string()).unwrap(),
                })
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::*;

    use std::sync::Once;

    static START: Once = Once::new();

    fn init() {
        START.call_once(|| {
            // run initialization here
            std::env::set_var("RUST_LOG", "trace");
            pretty_env_logger::init();
        });
    }
    fn test() -> Result<Dynamic, Box<EvalAltResult>> {
        to_dynamic(User { id: 1 })
    }

    #[derive(Serialize, Deserialize, PartialEq, Clone, Copy, Debug)]
    struct User {
        id: u32,
    }

    impl From<ScriptResult> for User {
        fn from(sr: ScriptResult) -> Self {
            sr.to().unwrap()
        }
    }
    #[tokio::test(threaded_scheduler)]
    async fn script_host() {
        init();
        let mut sh = ScriptEngine::with_default_plugins();
        let script = r#"
        print ("Hello" + " World");
        "test"
       "#;

        let result = sh
            .execute(Script::with_name("test script", &script))
            .unwrap();

        assert_eq!("test", result.to::<String>().unwrap_or_default());
    }
    #[tokio::test(threaded_scheduler)]
    async fn script_host_adv() {
        init();
        let mut sh = ScriptEngine::with_default_plugins();
        let script = r#"
        print ("Advanced Test");
        #{
            "id": 42
        }
       "#;

        let result = sh
            .execute(Script::with_name("test script", &script))
            .unwrap();

        assert_eq!(
            User { id: 42 },
            result.to::<User>().expect("to convert to a user")
        );
    }
}
