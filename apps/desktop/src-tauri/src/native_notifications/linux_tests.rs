use super::*;
use std::{
    sync::{
        Mutex as StdMutex,
        atomic::{AtomicU32, Ordering},
    },
    time::Duration,
};
use zbus::{connection::Builder, object_server::SignalEmitter, zvariant::OwnedValue};

#[derive(Default)]
struct Calls {
    notifications: Vec<(u32, String, String)>,
    closed: Vec<u32>,
    fail_close: bool,
    plain_text_only: bool,
}

struct Desktop {
    next: AtomicU32,
    calls: Arc<StdMutex<Calls>>,
}

#[zbus::interface(name = "org.freedesktop.Notifications")]
impl Desktop {
    fn get_capabilities(&self) -> Vec<&str> {
        if self.calls.lock().unwrap().plain_text_only {
            vec!["body"]
        } else {
            vec!["actions", "body", "body-markup"]
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn notify(
        &self,
        app_name: &str,
        replaces_id: u32,
        _icon: &str,
        title: &str,
        body: &str,
        actions: Vec<String>,
        hints: HashMap<String, OwnedValue>,
        _timeout: i32,
    ) -> u32 {
        assert_eq!(app_name, "Workman");
        assert_eq!(replaces_id, 0);
        if self.calls.lock().unwrap().plain_text_only {
            assert!(actions.is_empty());
        } else {
            assert_eq!(actions, ["default", "Open Workman"]);
        }
        assert!(hints.contains_key("desktop-entry"));
        let id = self.next.fetch_add(1, Ordering::SeqCst) + 1;
        self.calls
            .lock()
            .unwrap()
            .notifications
            .push((id, title.into(), body.into()));
        id
    }

    async fn close_notification(
        &self,
        id: u32,
        #[zbus(signal_emitter)] emitter: SignalEmitter<'_>,
    ) -> zbus::fdo::Result<()> {
        {
            let mut calls = self.calls.lock().unwrap();
            if calls.fail_close {
                return Err(zbus::fdo::Error::Failed("try again".into()));
            }
            calls.closed.push(id);
        }
        Self::notification_closed(&emitter, id, 3).await?;
        Ok(())
    }

    #[zbus(signal)]
    async fn notification_closed(
        emitter: &SignalEmitter<'_>,
        id: u32,
        reason: u32,
    ) -> zbus::Result<()>;
    #[zbus(signal)]
    async fn action_invoked(emitter: &SignalEmitter<'_>, id: u32, action: &str)
    -> zbus::Result<()>;
}

async fn desktop(calls: Arc<StdMutex<Calls>>) -> Connection {
    Builder::session()
        .unwrap()
        .name(SERVICE)
        .unwrap()
        .serve_at(
            PATH,
            Desktop {
                next: AtomicU32::new(0),
                calls,
            },
        )
        .unwrap()
        .build()
        .await
        .unwrap()
}

#[tokio::test]
#[ignore = "requires an isolated bus: dbus-run-session -- cargo test -p workman-desktop desktop_delivery_and_clear -- --ignored"]
async fn desktop_delivery_and_clear_survive_errors_and_server_replacement() {
    let calls = Arc::new(StdMutex::new(Calls::default()));
    let service = desktop(calls.clone()).await;
    let backend = Backend::default();
    assert!(
        backend
            .capabilities()
            .await
            .unwrap()
            .iter()
            .any(|capability| capability == "actions")
    );
    backend
        .show(11, "Agent finished", "A & B <C>", || {})
        .await
        .unwrap();
    let (opened, clicked) = tokio::sync::oneshot::channel();
    let opened = StdMutex::new(Some(opened));
    backend
        .show(12, "Agent needs input", "Second agent", move || {
            if let Some(opened) = opened.lock().unwrap().take() {
                let _ = opened.send(());
            }
        })
        .await
        .unwrap();
    assert_eq!(
        calls.lock().unwrap().notifications[0].2,
        "A &amp; B &lt;C&gt;"
    );
    backend.dismiss(&[11]).await.unwrap();
    assert_eq!(calls.lock().unwrap().closed, [1]);
    assert!(backend.shown.lock().await.contains_key(&12));
    let emitter = SignalEmitter::new(&service, PATH).unwrap();
    Desktop::action_invoked(&emitter, 2, "default")
        .await
        .unwrap();
    tokio::time::timeout(Duration::from_secs(3), clicked)
        .await
        .unwrap()
        .unwrap();

    backend
        .show(13, "Retry removal", "Third agent", || {})
        .await
        .unwrap();
    calls.lock().unwrap().fail_close = true;
    assert!(backend.dismiss(&[13]).await.is_err());
    assert!(backend.shown.lock().await.contains_key(&13));
    calls.lock().unwrap().fail_close = false;
    backend.dismiss(&[13]).await.unwrap();
    backend.dismiss(&[13]).await.unwrap();
    assert_eq!(
        calls
            .lock()
            .unwrap()
            .closed
            .iter()
            .filter(|id| **id == 3)
            .count(),
        1
    );

    backend
        .show(14, "Old desktop", "Older unread", || {})
        .await
        .unwrap();
    service.release_name(SERVICE).await.unwrap();
    let new_calls = Arc::new(StdMutex::new(Calls::default()));
    let _replacement = desktop(new_calls.clone()).await;
    backend
        .show(15, "New desktop", "Newer unread", || {})
        .await
        .unwrap();
    backend.dismiss(&[14]).await.unwrap();
    assert!(calls.lock().unwrap().closed.contains(&4));
    assert!(
        new_calls.lock().unwrap().closed.is_empty(),
        "the old read must never close an ID reused by the new desktop"
    );
    backend.dismiss(&[15]).await.unwrap();
    assert_eq!(new_calls.lock().unwrap().closed, [1]);

    new_calls.lock().unwrap().plain_text_only = true;
    backend
        .show(16, "Minimal desktop", "A & B <C>", || {})
        .await
        .unwrap();
    assert_eq!(new_calls.lock().unwrap().notifications[1].2, "A & B <C>");
    backend.dismiss(&[16]).await.unwrap();
}
