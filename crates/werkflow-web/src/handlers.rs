
use serde_json::Value;
use werkflow_scripting::to_dynamic;
use werkflow_agents::work::CommandHostPlugin;
use werkflow_datalayer::cache::RemoteStoragePlugin;
use anyhow::{anyhow, Result};
use handlebars::Handlebars;
use log::{debug, info, warn};
use rand::Rng;
use std::convert::Infallible;
use std::{sync::Arc};
use tokio::stream::StreamExt;
use tokio::sync::RwLock;
use warp::{Buf, Reply, hyper::body::Bytes};
use warp::{Rejection, Stream, http::Response};
use werkflow_agents::{AgentCommand, AgentController, plugins::http, work::{Workload, WorkloadStatus}};
use werkflow_scripting::{RegisterFn, Script, ScriptEngine, state::HostState};
use lazy_static::lazy_static;

use crate::{rhtml::Library, model};

pub async fn metrics_handler() -> Result<impl Reply, Rejection> {
    use prometheus::Encoder;
    let encoder = prometheus::TextEncoder::new();

    let mut buffer = Vec::new();
    if let Err(e) = encoder.encode(&crate::prom::REGISTRY.gather(), &mut buffer) {
        eprintln!("could not encode custom metrics: {}", e);
    };
    let mut res = match String::from_utf8(buffer.clone()) {
        Ok(v) => v,
        Err(e) => {
            eprintln!("custom metrics could not be from_utf8'd: {}", e);
            String::default()
        }
    };
    buffer.clear();

    let mut buffer = Vec::new();
    if let Err(e) = encoder.encode(&prometheus::gather(), &mut buffer) {
        eprintln!("could not encode prometheus metrics: {}", e);
    };
    let res_custom = match String::from_utf8(buffer.clone()) {
        Ok(v) => v,
        Err(e) => {
            eprintln!("prometheus metrics could not be from_utf8'd: {}", e);
            String::default()
        }
    };
    buffer.clear();

    res.push_str(&res_custom);
    Ok(res)
}

pub async fn print_status<'a>(controller: AgentController) -> Result<impl warp::Reply, Infallible> {
    let agent = controller.agent.read().await;
    Ok(format!(
        "{} The current status is: {:?}",
        agent.name,
        agent.status()
    ))
}

pub async fn start_job<'a>(
    controller: AgentController,
    script: Script,
) -> Result<impl warp::Reply, Infallible> {
    let mut agent = controller.agent.write().await;

    let wl_handle = agent.run(Workload::with_script(controller.clone(), script));

    agent.runtime.spawn(async move {
        let id = wl_handle.read().await.id;
        let mut handle = wl_handle.write().await;

        if handle.status == WorkloadStatus::None {
            return;
        }

        let jh = handle.join_handle.take();

        if let Some(h) = jh {
            match h
                .await
                .map_err(|err| anyhow!("Could not join job thread. {}", err))
                .unwrap()
            {
                Ok(result) => {
                    info!("Job {} completed. Result: {}", id, result);
                    handle.result = Some(result);

                    handle.status = WorkloadStatus::Complete;
                }
                Err(err) => {
                    let err_string = err.to_string().clone();
                    anyhow!("Workload error thrown for job {}: {}", id, err_string);
                    handle.status = WorkloadStatus::Error(err_string);
                }
            }
        }

        drop(handle);
    });

    Ok(format!("Started job"))
}

pub async fn list_jobs<'a>(controller: AgentController) -> Result<impl warp::Reply, Infallible> {
    info!("Read lock on agent");
    let work = controller.agent.read().await.work_handles.clone();
    info!("Got Read lock on agent");
    let mut vec: Vec<model::JobResult> = Vec::new();

    for jh in work {
        debug!("Read lock on job handle");
        let wh = jh.read().await;

        vec.push(model::JobResult {
            id: wh.id,
            status: wh.status.to_string(),
            result_string: wh.result.clone().unwrap_or_default(),
        });
        debug!("Done read lock on job handle");

        drop(wh);
    }

    Ok(serde_json::to_string(&vec).unwrap())
}

pub async fn stop_agent(controller: AgentController) -> Result<impl warp::Reply, Infallible> {
    let mut agent = controller.agent.write().await;

    agent.work_handles.clear();

    agent.command(AgentCommand::Stop).await;

    Ok(format!("The agent has been stopped."))
}

pub async fn start_agent(controller: AgentController) -> Result<impl warp::Reply, Infallible> {
    let _ = controller.agent.write().await.command(AgentCommand::Start).await;

    Ok(format!("The agent has been started."))
}
lazy_static! {
    pub static ref ENGINE : RwLock<ScriptEngine<'static>> = {
        let mut s = ScriptEngine::with_default_plugins();
        s.add_plugin(http::Plugin)
        .add_plugin(RemoteStoragePlugin)
        .add_plugin(CommandHostPlugin);
        RwLock::new(s)
    };
    pub static ref TEMPLATES: RwLock<Handlebars<'static>> = {
        RwLock::new(Handlebars::new())
    };
}

pub async fn process_template(
    template_name: String,
    body : Bytes,
    state: Arc<RwLock<HostState>>,    
    library: Arc<RwLock<Library>>,
    content_type: String
) -> Result<impl warp::Reply, Infallible>
{

    let mut engine = ScriptEngine::with_default_plugins();
    
    engine.add_plugin(http::Plugin)
        .add_plugin(RemoteStoragePlugin)
        .add_plugin(CommandHostPlugin);

    let script_template = match library.read().await.get(&template_name) {
        Ok(template) => {
            template
        }
        Err(err) => {
            warn!("Error getting template {}. Error: {}", template_name, err);
            return Ok(Response::builder()
            .status(404)
            .body("Four Oh Four".to_string()));
        }
    };
    let mut template_writer =  TEMPLATES.write().await;

    if !template_writer.has_template(&template_name) {
        if let Err(err) = template_writer.register_template_string(&template_name, &script_template.template) {
            warn!("Error registering template {}\n Template:\n{}", err, script_template.template);
            return Ok(Response::builder()
            .status(500)
            .body("We ain't found shit.".to_string()));
        }
    }
    
    drop(template_writer);
    // lock the state so no other thread can update it while we're processing.
    let mut state = state.write().await;
    
    let bytes = body.bytes().to_vec();

    let body = std::str::from_utf8(&bytes).expect("error converting bytes to &str").to_string();
    
    engine.scope.push("state", state.clone());
    engine.engine.on_var(move |name,_,_| {
        if name == "body" {
            let json : Value = serde_json::from_str(&body.clone()).unwrap();
            return Ok(Some(to_dynamic(json).unwrap()));
        }
        Ok(None)
    });
    // can clean with ammonia to get rid of XSS and other potentially dangerous things
    /*match &content_type[..] {
        "application/json" => engine.scope.push("body", Arc::new(to_dynamic(body).unwrap())),
        _ => engine.scope.push("body", String::from( ammonia::clean(body)))
    };*/
    
    let result = engine

        .execute(script_template.script.clone());

    match result {
        Ok(result) => {
            *state = engine.scope.get_value("state").unwrap();

            drop(state);
        
            // Render the HTML
            let html = TEMPLATES.read().await
                .render(&template_name, &result.underlying)
                .unwrap();
            
            //let clean_html = ammonia::clean(&html).clone();
        
            Ok(Response::builder()
                .header("Content-Type", "text/html")
                .body(html)) 
        }
        Err(err) => {
            drop(state);

            let error_id : u128 = rand::thread_rng().gen();
            warn!("Error running script error: {}\n ErrorId: {} Script:\n{}", err, error_id, script_template.script.body);
            return Ok(Response::builder()
            .status(500)
            .body(format!("You done f'd up. ErrorId: {}", error_id)));
        }
    }
}
