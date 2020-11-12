use log::info;
use werkflow_agents::{AgentController, work::Workload, AgentCommand};
use warp::Rejection;
use warp::Reply;
use werkflow_scripting::Script;
use anyhow::{anyhow, Result};
use std::convert::Infallible;

use crate::model;

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

pub async fn print_status<'a>(
    controller: AgentController,
) -> Result<impl warp::Reply, Infallible> {
    Ok(format!(
        "{} The current status is: {:?}",
        controller.agent.read().name,
        controller.agent.read().status()
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
        info!("Starting job {}", id);
        let mut handle = wl_handle.write().await;
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
                    
                    handle.status = werkflow_agents::work::WorkloadStatus::Complete;
                }
                Err(err) => {
                    let err_string = err.to_string().clone();
                    anyhow!("Workload error thrown for job {}: {}", id, err_string);
                    handle.status = werkflow_agents::work::WorkloadStatus::Error(err_string);
                }
            }
            
        }

        drop(handle);
    });

    Ok(format!("Started job"))
}

pub async fn list_jobs<'a>(
    controller: AgentController,
) -> Result<impl warp::Reply, Infallible> {
    info!("Read lock on agent");
    let work = controller.agent.read().work_handles.clone();
    info!("Got Read lock on agent");
    let mut vec: Vec<model::JobResult> = Vec::new();

    for jh in work {
        info!("Read lock on job handle");
        let wh = jh.read().await;

        vec.push(model::JobResult {
            id: wh.id,
            status: wh.status.to_string(),
            result_string: wh.result.clone().unwrap_or_default(),
        });
        info!("Done read lock on job handle");

        drop(wh);
    }

    Ok(serde_json::to_string(&vec).unwrap())
}

pub async fn stop_agent(controller: AgentController) -> Result<impl warp::Reply, Infallible> {
    let mut agent = controller.agent.write();

    let _ = agent.command(AgentCommand::Stop).unwrap();

    agent.work_handles.clear();

    Ok(format!("The agent has been stopped."))
}
pub async fn start_agent(controller: AgentController) -> Result<impl warp::Reply, Infallible> {
    let _ = controller
        .agent
        .write()
        .command(AgentCommand::Start)
        .unwrap();

    Ok(format!("The agent has been started."))
}