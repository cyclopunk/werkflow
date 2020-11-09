use log::{debug, info};
use rand::prelude::*;
use rhai::Scope;
use std::fmt;

pub use rhai::serde::*;

pub use rhai::{
    Dynamic, Engine, EvalAltResult, ImmutableString, Map, Position, RegisterFn, RegisterResultFn,
};

use serde::{Deserialize, Serialize};

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
    identifier: ScriptIdentifier,
    body: String,
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
pub struct ScriptHost<'a> {
    pub engine: Engine,
    pub scope: Scope<'a>,
}

#[derive(PartialEq, Clone, Serialize, Deserialize, Default)]
pub struct ScriptHostError {
    file: String,
    line: usize,
    error_text: String,
}

// Implement std::fmt::Display for AppError
impl fmt::Display for ScriptHostError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "An Error Occurred, Please Try Again!") // user-facing output
    }
}

// Implement std::fmt::Debug for AppError
impl fmt::Debug for ScriptHostError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{{ file: {}, line: {} }}", self.file, self.line) // programmer-facing output
    }
}

#[derive(Debug, Clone)]
pub struct ScriptResult {
    pub underlying: Dynamic,
}

impl ScriptResult {
    pub fn to<T>(&self) -> Result<T, ScriptHostError>
    where
        for<'de> T: Deserialize<'de>,
    {
        let bare_str = self.underlying.as_str().unwrap_or_default();

        if self.underlying.is::<String>() {
            if let Ok(obj) = serde_json::from_str(bare_str) {
                Ok(obj)
            } else {
                // support for returned raw strings
                Ok(serde_json::from_str(&format!("\"{}\"", bare_str)).unwrap())
            }
        } else {
            from_dynamic::<T>(&self.underlying).map_err(|_err| ScriptHostError {
                error_text: format!("Could not deserialize {} into struct", bare_str).into(),
                ..Default::default()
            })
        }
    }
}

impl<'a> ScriptHost<'a> {
    pub fn new() -> ScriptHost<'a> {
        ScriptHost {
            scope: Scope::new(),
            engine: Engine::new(),
        }
    }

    pub fn with_engine<T>(&mut self, func: T)
    where
        for<'fo> T: FnOnce(&'fo mut Engine),
    {
        func(&mut self.engine);
    }

    pub fn register_type<T: Sync + Send + Clone + 'static>(&'a mut self) {
        self.engine.register_type::<T>();
    }

    pub async fn execute(&'a self, script: Script) -> Result<ScriptResult, ScriptHostError> {
        debug!("Start running script in Script Host:\n {:?}", script);

        let d = self
            .engine
            .eval::<Dynamic>(&script.body)
            .map_err(|err| ScriptHostError {
                error_text: err.to_string(),
                line: err.position().position().unwrap_or_default(),
                file: script.identifier.name.clone(),
            });

        info!(
            "Done running script {} in Script Host",
            script.identifier.name
        );

        match d {
            Ok(r) => Ok(ScriptResult { underlying: r }),
            Err(err) => {
                info!("Error running script {:?}", err);
                Err(err)
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
        let sh = ScriptHost::new();
        let script = r#"
        print ("Hello" + " World");
        "test"
       "#;

        let result = sh
            .execute(Script::with_name("test script", &script))
            .await
            .unwrap();

        assert_eq!("test", result.to::<String>().unwrap_or_default());
    }
    #[tokio::test(threaded_scheduler)]
    async fn script_host_adv() {
        init();
        let sh = ScriptHost::new();
        let script = r#"
        print ("Advanced Test");
        #{
            "id": 42
        }
       "#;

        let result = sh
            .execute(Script::with_name("test script", &script))
            .await
            .unwrap();

        assert_eq!(
            User { id: 42 },
            result.to::<User>().unwrap_or(User { id: 0 })
        );
    }
}
