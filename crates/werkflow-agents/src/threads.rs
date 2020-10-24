use std::future::Future;
use anyhow::anyhow;

pub(crate) struct AsyncRunner;

impl AsyncRunner {
    pub(crate) fn block_on<'a, F: Future + Sync + Send + 'static>(f: F) -> F::Output where F::Output : Sync + Send + 'static { 
        let task = tokio::task::spawn_blocking(move || futures::executor::block_on( f ) );

        futures::executor::block_on( task ) 
            .map_err(|err| anyhow!("Couldn't run future")).unwrap()
    }
}