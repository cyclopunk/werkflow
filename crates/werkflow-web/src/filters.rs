use tokio::sync::RwLock;
use warp::Filter;
use werkflow_scripting::{HostState, Script};
use std::{sync::Arc, collections::HashMap, convert::Infallible};

use crate::{AgentController, handlers};

fn with_agent(
    agent: AgentController,
) -> impl Filter<Extract = (AgentController,), Error = std::convert::Infallible> + Clone {
    warp::any().map(move || agent.clone())
}
fn with_state( state: Arc<RwLock<HostState>>
) -> impl Filter<Extract = (Arc<RwLock<HostState>>,), Error = std::convert::Infallible> + Clone {
    warp::any().map(move || state.clone())
}
pub fn metrics() -> impl Filter<Extract = impl warp::Reply, Error = warp::Rejection> + Clone {
    warp::path!("metrics")
        .and(warp::get())
        .and_then(handlers::metrics_handler)
}

pub fn agent_status(
    agent: AgentController,
) -> impl Filter<Extract = impl warp::Reply, Error = warp::Rejection> + Clone {
    warp::path!("status")
        .and(warp::get())
        .and(warp::any().map(move || agent.clone()))
        .and_then(handlers::print_status)
}

pub fn start_job(
    agent: AgentController,
) -> impl Filter<Extract = impl warp::Reply, Error = warp::Rejection> + Clone {
    warp::path!("run")
        .and(warp::post())
        .and(warp::any().map(move || agent.clone()))
        .and(warp::filters::body::json::<Script>())
        .and_then(handlers::start_job)
}

pub fn stop_agent(
    agent: AgentController,
) -> impl Filter<Extract = impl warp::Reply, Error = warp::Rejection> + Clone {
    warp::path!("stop")
        .and(warp::post())
        .and(with_agent(agent))
        .and_then(handlers::stop_agent)
}

pub fn start_agent(
    agent: AgentController,
) -> impl Filter<Extract = impl warp::Reply, Error = warp::Rejection> + Clone {
    warp::path!("start")
        .and(warp::post())
        .and(with_agent(agent))
        .and_then(handlers::start_agent)
}

pub fn list_jobs(
    agent: AgentController,
) -> impl Filter<Extract = impl warp::Reply, Error = warp::Rejection> + Clone {
    warp::path!("jobs")
        .and(warp::get())
        .and(with_agent(agent))
        .and_then(handlers::list_jobs)
}


pub fn templates(
    agent: AgentController,
    state: Arc<RwLock<HostState>>
) -> impl Filter<Extract = impl warp::Reply, Error = warp::Rejection> + Clone {
    warp::path!("content" / String )
        .and(warp::post())
        .and(warp::body::stream())
        .and(with_state(state))
        .and_then(handlers::process_template)
}