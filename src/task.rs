use async_std::{future, task};
use std::time::Duration;
use anyhow::Result;

/// delay a future by a duration
pub fn delay_task<F>(fut: F, dur: Duration) -> task::JoinHandle<Result<()>>
where
	F: future::Future<Output = Result<()>> + Send + 'static,
{
	task::spawn(async move {
		let forever = future::pending::<()>();
		// ignore the TimeOut error because forever is staying forever in pending state
		let _ = future::timeout(dur, forever).await;
		fut.await?;
		Ok::<(), anyhow::Error>(())
	})
}
