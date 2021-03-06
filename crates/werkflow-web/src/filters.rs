use std::sync::Arc;
use tokio::sync::RwLock;
use warp::Filter;
use werkflow_scripting::{Script, state::HostState, scheduler::ScriptScheduler};

use crate::{handlers, rhtml::Library, AgentController};

fn with_agent(
    agent: AgentController,
) -> impl Filter<Extract = (AgentController,), Error = std::convert::Infallible> + Clone {
    warp::any().map(move || agent.clone())
}
fn with_scheduler(
    scheduler: Arc<RwLock<ScriptScheduler>>,
) -> impl Filter<Extract = (Arc<RwLock<ScriptScheduler>>,), Error = std::convert::Infallible> + Clone {
    warp::any().map(move || scheduler.clone())
}
fn with_state(
    state: Arc<RwLock<HostState>>,
) -> impl Filter<Extract = (Arc<RwLock<HostState>>,), Error = std::convert::Infallible> + Clone {
    warp::any().map(move || state.clone())
}
fn with_library(
    library: Arc<RwLock<Library>>,
) -> impl Filter<Extract = (Arc<RwLock<Library>>,), Error = std::convert::Infallible> + Clone {
    warp::any().map(move || library.clone())
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

pub fn templates<'a>(
    _agent: AgentController,
    state: Arc<RwLock<HostState>>,
    library: Arc<RwLock<Library>>,
) -> impl Filter<Extract = impl warp::Reply, Error = warp::Rejection> + Clone + 'a {
    warp::path!("content" / String)
        .and(warp::any())
        .and(warp::filters::body::bytes())
        .and(with_state(state))
        .and(with_library(library.clone()))
        .and(warp::header::<String>("content-type"))
        .and_then(handlers::process_template)
}



pub fn schedule<'a>(
    agent: AgentController,
    state: Arc<RwLock<HostState>>,
    scheduler: Arc<RwLock<ScriptScheduler>>,
) -> impl Filter<Extract = impl warp::Reply, Error = warp::Rejection> + Clone + 'a {
    warp::path!("content")
        .and(warp::any())
        .and(with_agent(agent))
        .and(with_scheduler(scheduler))
        .and(warp::filters::body::bytes())
        .and(with_state(state))
        .and(warp::header::<String>("content-type"))
        .and_then(handlers::schedule)
}
