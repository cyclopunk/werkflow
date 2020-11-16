
use anyhow::{anyhow, Result};
use handlebars::Handlebars;
use log::{debug, info};
use std::convert::Infallible;
use std::{sync::Arc};
use tokio::stream::StreamExt;
use tokio::sync::RwLock;
use warp::Reply;
use warp::{Rejection, Stream, http::Response};
use werkflow_agents::{
    work::{Workload, WorkloadStatus},
    AgentCommand, AgentController,
};
use werkflow_scripting::{state::HostState, Script, ScriptEngine};

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
    let agent = controller.agent.read();
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
    let mut agent = controller.agent.write();

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
    let work = controller.agent.read().work_handles.clone();
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
    let agent_arc = controller.agent.clone();
    let mut agent = agent_arc.write();

    agent.work_handles.clear();

    agent.command(AgentCommand::Stop);

    Ok(format!("The agent has been stopped."))
}

pub async fn start_agent(controller: AgentController) -> Result<impl warp::Reply, Infallible> {
    let _ = controller.agent.write().command(AgentCommand::Start);

    Ok(format!("The agent has been started."))
}

pub async fn process_code_stream<S, B>(
    template_name: String,
    script: S,
    state: Arc<RwLock<HostState>>,
    handlebars: Arc<handlebars::Handlebars<'_>>
) -> Result<impl warp::Reply, Infallible>
where
    S: Stream<Item = Result<B, warp::Error>>,
    S: StreamExt,
    B: warp::Buf,
{

    // Get the script from the input stream
    
    let mut pinned_stream = Box::pin(script);

    let mut script_txt = String::new();

    while let Some(item) = pinned_stream.next().await {
        let mut data = item.unwrap();
        script_txt.push_str(&String::from_utf8(data.to_bytes().as_ref().to_vec()).unwrap());
    }

    let mut script_engine = ScriptEngine::with_default_plugins();
    
    // lock the state so no other thread can update it while we're processing.
    let mut state = state.write().await;

    script_engine.scope.push("state", state.clone());

    let result = script_engine
        .execute(Script::with_name(&template_name[..], &script_txt))
        .unwrap();

        // update the state
    *state = script_engine.scope.get_value("state").unwrap();

    drop(state);

    // Render the HTML
    let html = handlebars
        .render(&template_name, &result.underlying)
        .unwrap();

    Ok(Response::builder()
        .header("Content-Type", "text/html")
        .body(ammonia::clean(&html))) // clean with ammonia to get rid of XSS and other potentially dangerous things
}

pub async fn process_template(
    template_name: String,
    state: Arc<RwLock<HostState>>,    
    library: Library
) -> Result<impl warp::Reply, Infallible>
{

    let mut hb = Handlebars::new();
/*
    let mut script_engine = ScriptEngine::with_default_plugins();
    
    // lock the state so no other thread can update it while we're processing.
    let mut state = state.write().await;

    script_engine.scope.push("state", state.clone());

    let result = script_engine
        .execute(Script::with_name(&template_name[..], &script_txt))
        .unwrap();

        // update the state
    *state = script_engine.scope.get_value("state").unwrap();

    drop(state);

    // Render the HTML
    let html = handlebars
        .render(&template_name, &result.underlying)
        .unwrap();
*/
    let html = "";
    Ok(Response::builder()
        .header("Content-Type", "text/html")
        .body(ammonia::clean(&html))) // clean with ammonia to get rid of XSS and other potentially dangerous things
}
