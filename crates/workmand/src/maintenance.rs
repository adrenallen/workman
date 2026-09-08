//! Periodic housekeeping runs on the blocking pool, never on the terminal input loop.
use crate::SharedProcessRegistry;
use std::{path::PathBuf, time::Duration};
use tokio::{
    sync::watch,
    task::JoinHandle,
    time::{MissedTickBehavior, interval},
};

pub(crate) fn spawn_storage_maintenance(
    registry: SharedProcessRegistry,
    data_dir: PathBuf,
    mut shutdown: watch::Receiver<bool>,
) -> JoinHandle<()> {
    tokio::spawn(async move {
        let mut tick = interval(Duration::from_secs(3600));
        tick.set_missed_tick_behavior(MissedTickBehavior::Skip);
        loop {
            tokio::select! {
                biased;
                _ = shutdown.changed() => break,
                _ = tick.tick() => {
                    let registry = registry.clone();
                    let data_dir = data_dir.clone();
                    let result = tokio::task::spawn_blocking(move || {
                        let registry = registry.blocking_lock();
                        registry.maintain_storage().map_err(|error| error.to_string())?;
                        crate::recorded_feedback::sweep_orphaned_media(registry.store(), &data_dir).map_err(|error| error.to_string())
                    }).await;
                    match result {
                        Ok(Ok(())) => {},
                        Ok(Err(error)) => eprintln!("workman storage maintenance: {error}"),
                        Err(error) => eprintln!("workman storage maintenance task: {error}"),
                    }
                }
            }
        }
    })
}
