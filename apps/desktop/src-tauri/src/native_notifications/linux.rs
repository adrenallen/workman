//! Freedesktop notifications, scoped to the unique D-Bus server that issued each ID.
use std::{collections::HashMap, sync::Arc};

use futures_util::StreamExt;
use tokio::sync::{Mutex, Notify};
use zbus::{Connection, Proxy, fdo::DBusProxy, names::BusName, zvariant::Value};

const SERVICE: &str = "org.freedesktop.Notifications";
const PATH: &str = "/org/freedesktop/Notifications";

#[cfg(test)]
#[path = "linux_tests.rs"]
mod tests;

#[derive(Clone, Default)]
pub struct Backend {
    shown: Arc<Mutex<HashMap<i64, Delivered>>>,
}

#[derive(Clone)]
struct Delivered {
    connection: Connection,
    owner: String,
    id: u32,
    cancelled: Arc<Notify>,
}

async fn server() -> Result<(Connection, String, Vec<String>), String> {
    let connection = Connection::session()
        .await
        .map_err(|error| error.to_string())?;
    let bus = DBusProxy::new(&connection)
        .await
        .map_err(|error| error.to_string())?;
    // Let D-Bus activate the desktop service if needed, then pin its unique owner. A restarted
    // server may reuse numeric IDs; stale handles must never close another app's notification.
    let proxy = Proxy::new(&connection, SERVICE, PATH, SERVICE)
        .await
        .map_err(|error| error.to_string())?;
    let capabilities: Vec<String> = proxy
        .call("GetCapabilities", &())
        .await
        .map_err(|error| error.to_string())?;
    let owner = bus
        .get_name_owner(BusName::try_from(SERVICE).unwrap())
        .await
        .map_err(|error| error.to_string())?;
    Ok((connection, owner.to_string(), capabilities))
}

impl Backend {
    pub async fn capabilities(&self) -> Result<Vec<String>, String> {
        server().await.map(|(_, _, capabilities)| capabilities)
    }

    pub async fn show(
        &self,
        notification_id: i64,
        title: &str,
        body: &str,
        on_open: impl Fn() + Send + 'static,
    ) -> Result<(), String> {
        let (connection, owner, capabilities) = server().await?;
        let proxy = Proxy::new(&connection, owner.as_str(), PATH, SERVICE)
            .await
            .map_err(|error| error.to_string())?;
        // Subscribe before Notify so a quick click cannot race registration of the callback.
        let mut actions = proxy
            .receive_signal("ActionInvoked")
            .await
            .map_err(|error| error.to_string())?;
        let mut closed = proxy
            .receive_signal("NotificationClosed")
            .await
            .map_err(|error| error.to_string())?;
        let hints = HashMap::from([
            ("desktop-entry", Value::from("workman-desktop")),
            ("transient", Value::from(false)),
        ]);
        let body = if capabilities
            .iter()
            .any(|capability| capability == "body-markup")
        {
            body.replace('&', "&amp;")
                .replace('<', "&lt;")
                .replace('>', "&gt;")
        } else {
            body.to_owned()
        };
        let default_action = if capabilities
            .iter()
            .any(|capability| capability == "actions")
        {
            vec!["default", "Open Workman"]
        } else {
            Vec::new()
        };
        let id: u32 = proxy
            .call(
                "Notify",
                &(
                    "Workman",
                    0_u32,
                    "workman-desktop",
                    title,
                    body,
                    default_action,
                    hints,
                    -1_i32,
                ),
            )
            .await
            .map_err(|error| error.to_string())?;
        let cancelled = Arc::new(Notify::new());
        let delivered = Delivered {
            connection: connection.clone(),
            owner: owner.clone(),
            id,
            cancelled: cancelled.clone(),
        };
        let previous = self.shown.lock().await.insert(notification_id, delivered);
        if let Some(previous) = previous {
            let _ = previous.close().await;
            previous.cancelled.notify_one();
        }
        let shown = self.shown.clone();
        tokio::spawn(async move {
            let mut opened = false;
            loop {
                tokio::select! {
                    _ = cancelled.notified() => break,
                    signal = actions.next() => {
                        let Some(signal) = signal else { break };
                        if let Ok((signal_id, action)) = signal.body().deserialize::<(u32, String)>()
                            && signal_id == id && action == "default"
                        {
                            on_open();
                            opened = true;
                            break;
                        }
                    }
                    signal = closed.next() => {
                        let Some(signal) = signal else { break };
                        if signal.body().deserialize::<(u32, u32)>().is_ok_and(|(signal_id, _)| signal_id == id) {
                            break;
                        }
                    }
                }
            }
            let item = {
                let mut shown = shown.lock().await;
                if shown
                    .get(&notification_id)
                    .is_some_and(|item| item.id == id && item.owner == owner)
                {
                    shown.remove(&notification_id)
                } else {
                    None
                }
            };
            if opened && let Some(item) = item {
                let _ = item.close().await;
            }
        });
        Ok(())
    }

    pub async fn dismiss(&self, notification_ids: &[i64]) -> Result<(), String> {
        for notification_id in notification_ids {
            let item = self.shown.lock().await.get(notification_id).cloned();
            if let Some(item) = item {
                item.close().await?;
                item.cancelled.notify_one();
                let mut shown = self.shown.lock().await;
                if shown
                    .get(notification_id)
                    .is_some_and(|current| current.id == item.id && current.owner == item.owner)
                {
                    shown.remove(notification_id);
                }
            }
        }
        Ok(())
    }
}

impl Delivered {
    async fn close(&self) -> Result<(), String> {
        let proxy = Proxy::new(&self.connection, self.owner.as_str(), PATH, SERVICE)
            .await
            .map_err(|error| error.to_string())?;
        match proxy
            .call::<_, _, ()>("CloseNotification", &(self.id))
            .await
        {
            Ok(()) => Ok(()),
            // The desktop has already dropped the notification, or its server has exited.
            Err(zbus::Error::MethodError(name, _, _))
                if matches!(
                    name.as_str(),
                    "org.freedesktop.DBus.Error.ServiceUnknown"
                        | "org.freedesktop.DBus.Error.NameHasNoOwner"
                        | "org.freedesktop.Notifications.Error.InvalidId"
                        | "org.freedesktop.DBus.Error.InvalidArgs"
                ) =>
            {
                Ok(())
            }
            Err(error) => Err(error.to_string()),
        }
    }
}
