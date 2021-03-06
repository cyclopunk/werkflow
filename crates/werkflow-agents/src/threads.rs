use anyhow::anyhow;
use std::future::Future;

pub struct AsyncRunner;

/// Helper methods for running async tasks in the tokio runtime.
impl AsyncRunner {
    pub fn block_on<'a, F: Future + Send + 'static>(f: F) -> F::Output
    where
        F::Output: Sync + Send + 'static,
    {
        let task = tokio::task::spawn_blocking(move || async_std::task::block_on(f));

        async_std::task::block_on(task)
            .map_err(|_err| anyhow!("Couldn't run future"))
            .unwrap()
    }
}
