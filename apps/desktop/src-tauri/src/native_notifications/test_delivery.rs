//! The test deadline belongs to the native runtime, so a suspended WebView cannot delay it.
use std::{
    future::Future,
    sync::{Arc, Mutex},
    time::Duration,
};

use serde::Serialize;

#[derive(Clone, Debug, Default, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Phase {
    #[default]
    Idle,
    Scheduled,
    Delivering,
    Sent,
    Error,
    Cancelled,
}

#[derive(Clone, Debug, Default, Serialize)]
pub struct Status {
    pub id: Option<String>,
    pub phase: Phase,
    pub error: Option<String>,
}

#[derive(Default)]
struct Pending {
    status: Status,
    task: Option<tauri::async_runtime::JoinHandle<()>>,
}

#[derive(Default)]
pub struct TestDelivery(Mutex<Pending>);

impl TestDelivery {
    pub fn status(&self) -> Status {
        self.0.lock().unwrap().status.clone()
    }

    pub fn cancel(&self, id: &str) -> Status {
        let mut pending = self.0.lock().unwrap();
        if pending.status.id.as_deref() == Some(id) && pending.status.phase == Phase::Scheduled {
            if let Some(task) = pending.task.take() {
                task.abort();
            }
            pending.status.phase = Phase::Cancelled;
        }
        pending.status.clone()
    }

    pub fn schedule<F>(
        self: &Arc<Self>,
        id: String,
        delay: Duration,
        deliver: F,
        completed: impl FnOnce(Status) + Send + 'static,
    ) -> Status
    where
        F: Future<Output = Result<(), String>> + Send + 'static,
    {
        let mut pending = self.0.lock().unwrap();
        if let Some(task) = pending.task.take() {
            if pending.status.phase == Phase::Scheduled {
                task.abort();
            }
        }
        pending.status = Status {
            id: Some(id.clone()),
            phase: Phase::Scheduled,
            error: None,
        };
        let state = self.clone();
        pending.task = Some(tauri::async_runtime::spawn(async move {
            tokio::time::sleep(delay).await;
            {
                let mut pending = state.0.lock().unwrap();
                if pending.status.id.as_deref() != Some(&id)
                    || pending.status.phase != Phase::Scheduled
                {
                    return;
                }
                // Native OS submission may outlive cancellation (macOS worker / Windows COM).
                // Once delivery starts, finish it and report its result instead of claiming it
                // was cancelled while the OS can still display the banner.
                pending.status.phase = Phase::Delivering;
            }
            let result = deliver.await;
            let status = {
                let mut pending = state.0.lock().unwrap();
                if pending.status.id.as_deref() != Some(&id)
                    || pending.status.phase != Phase::Delivering
                {
                    return;
                }
                pending.status.phase = if result.is_ok() {
                    Phase::Sent
                } else {
                    Phase::Error
                };
                pending.status.error = result.err();
                pending.task = None;
                pending.status.clone()
            };
            completed(status);
        }));
        pending.status.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn committed_delivery_finishes_and_reports_success_after_a_late_cancel() {
        let state = Arc::new(TestDelivery::default());
        let (entered, started) = tokio::sync::oneshot::channel();
        let (release, wait) = tokio::sync::oneshot::channel();
        let (done, result) = tokio::sync::oneshot::channel();
        state.schedule(
            "committed".into(),
            Duration::ZERO,
            async move {
                let _ = entered.send(());
                wait.await.unwrap();
                Ok(())
            },
            move |status| {
                let _ = done.send(status);
            },
        );
        tokio::time::timeout(Duration::from_secs(2), started)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(state.cancel("committed").phase, Phase::Delivering);
        release.send(()).unwrap();
        let status = tokio::time::timeout(Duration::from_secs(2), result)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(status.phase, Phase::Sent);
        assert_eq!(state.status().phase, Phase::Sent);
    }

    #[tokio::test]
    async fn delivers_without_a_webview_and_caches_completion() {
        let state = Arc::new(TestDelivery::default());
        let (send, receive) = tokio::sync::oneshot::channel();
        let start = std::time::Instant::now();
        let delay = Duration::from_millis(30);
        let status = state.schedule("test".into(), delay, async { Ok(()) }, move |status| {
            let _ = send.send(status);
        });
        assert_eq!(status.phase, Phase::Scheduled);
        let result = tokio::time::timeout(Duration::from_secs(2), receive)
            .await
            .unwrap()
            .unwrap();
        assert!(start.elapsed() >= delay);
        assert_eq!(result.phase, Phase::Sent);
        assert_eq!(state.status().phase, Phase::Sent);
    }

    #[tokio::test]
    async fn cancelled_and_replaced_tests_never_deliver_and_errors_are_retained() {
        let state = Arc::new(TestDelivery::default());
        let calls = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        for id in ["cancel", "replace"] {
            let calls = calls.clone();
            state.schedule(
                id.into(),
                Duration::from_millis(30),
                async move {
                    calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                    Ok(())
                },
                |_| {},
            );
            if id == "cancel" {
                assert_eq!(state.cancel(id).phase, Phase::Cancelled);
            }
        }
        let (send, receive) = tokio::sync::oneshot::channel();
        state.schedule(
            "latest".into(),
            Duration::from_millis(60),
            async { Err("Permission denied".into()) },
            move |status| {
                let _ = send.send(status);
            },
        );
        assert_eq!(state.cancel("replace").phase, Phase::Scheduled);
        let result = tokio::time::timeout(Duration::from_secs(2), receive)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(result.id.as_deref(), Some("latest"));
        assert_eq!(result.phase, Phase::Error);
        assert_eq!(state.status().error.as_deref(), Some("Permission denied"));
        assert_eq!(calls.load(std::sync::atomic::Ordering::SeqCst), 0);
    }
}
