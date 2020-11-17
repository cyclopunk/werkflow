pub mod http {
    use werkflow_scripting::{RegisterResultFn, to_dynamic};
    use werkflow_scripting::{Dynamic, EvalAltResult, Map};
    use werkflow_scripting::ScriptEnginePlugin;
    use log::debug;
    use anyhow::{anyhow, Result};

    use crate::{threads::AsyncRunner, work::SerMap};

    pub struct Plugin;

    impl ScriptEnginePlugin for Plugin {
        fn init(&self, host: &mut werkflow_scripting::ScriptEngine) {
            host.engine
                .register_result_fn("get", get)
                .register_result_fn("post", post)
                .register_result_fn("put", put)
                .register_result_fn("delete", delete);
        }
    }

    pub fn get(url: &'static str) -> Result<Dynamic, Box<EvalAltResult>> {
        debug!("Get {}", url);
        
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
    pub fn put(url: &'static str, body: Map) -> Result<Dynamic, Box<EvalAltResult>> {
        let l_url = url.clone();

        Ok(AsyncRunner::block_on(async_put(
            l_url.clone(),
            SerMap { underlying: body },
        )))
    }
    pub fn delete(url: &'static str) -> Result<Dynamic, Box<EvalAltResult>> {
        let l_url = url.clone();

        Ok(AsyncRunner::block_on(async_delete (
            l_url.clone()
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
    pub async fn async_delete(url: &str) -> Dynamic {
        let response = reqwest::Client::new()
            .delete(url)
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
    pub async fn async_put(url: &str, body: SerMap) -> Dynamic {
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