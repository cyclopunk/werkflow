use std::{any::type_name, fmt, sync::Arc};

pub use rhai::serde::*;

pub use rhai::{Dynamic, Engine, EvalAltResult, Map, Position, RegisterFn, RegisterResultFn};

use log::debug;
use serde::{Deserialize, Serialize};
#[derive(Debug, PartialEq, Clone, Serialize, Deserialize, Default)]
pub struct Script {
    body: String,
}

impl Script {
    pub fn new(script_body: &str) -> Script {
        return Script {
            body: script_body.to_string(),
        };
    }
}
pub struct ScriptHost {
    pub engine: Engine,
}
#[derive(PartialEq, Clone, Serialize, Deserialize)]
pub struct ScriptHostError {
    file: String,
    line: String,
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
    underlying: Dynamic,
}

impl ScriptResult {
    pub fn to<T>(&self) -> T
    where
        for<'de> T: Deserialize<'de>,
    {
        if self.underlying.is::<String>() {
            serde_json::from_str(self.underlying.as_str().unwrap()).unwrap()
        } else {
            from_dynamic::<T>(&self.underlying).unwrap()
        }
    }
}

impl ScriptHost {
    pub fn new() -> ScriptHost {
        ScriptHost {
            engine: Engine::new(),
        }
    }

    pub fn register_type<T: Sync + Send + Clone + 'static>(&mut self) {
        self.engine.register_type::<T>();
    }
    pub fn register_str_function(&mut self, name: &str, f: fn() -> String) {
        self.engine.register_fn(name, f);
    }
    pub fn register_dynamic_function(
        &mut self,
        name: &str,
        f: fn() -> Result<Dynamic, Box<EvalAltResult>>,
    ) {
        self.engine.register_result_fn(name, f);
    }
    pub async fn execute(&self, script: Script) -> Result<ScriptResult, ScriptHostError> {
        let d = self.engine.eval::<Dynamic>(&script.body);

        Ok(ScriptResult {
            underlying: d.unwrap(),
        })
    }
}

#[cfg(test)]
mod tests {
    use crate::*;
    use log::info;
    use tokio::runtime::Runtime;
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
    #[test]
    fn script_host() {
        let mut runtime = Runtime::new().unwrap();
        let mut host = ScriptHost::new();
        host.register_dynamic_function("test", test);

        init();

        let result = runtime.block_on(host
            .execute(Script {
                body: r#" test() "#.to_string(),
            }))
            .unwrap();

        info!("Running test");
        assert_eq!(User { id: 1 }, result.to());
    }
}
