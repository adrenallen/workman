use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager};

pub const ACTION_EVENT: &str = "notification://action";

#[derive(Clone, Debug, Serialize)]
pub struct NotificationPermission {
    state: &'static str,
    platform: &'static str,
    detail: Option<String>,
}

#[derive(Clone, Copy, Debug, Serialize)]
struct NotificationAction {
    notification_id: i64,
}

#[tauri::command]
pub async fn native_notification_permission_state() -> Result<NotificationPermission, String> {
    permission_state().await
}

#[tauri::command]
pub async fn native_notification_request_permission() -> Result<NotificationPermission, String> {
    request_permission().await
}

#[tauri::command]
pub async fn native_notification_show(
    app: AppHandle,
    notification_id: i64,
    title: String,
    body: String,
) -> Result<(), String> {
    let title = checked_copy("title", title, 160)?;
    let body = checked_copy("body", body, 1_024)?;

    show_notification(app, notification_id, title, body).await
}

#[cfg(target_os = "macos")]
async fn show_notification(
    app: AppHandle,
    notification_id: i64,
    title: String,
    body: String,
) -> Result<(), String> {
    let handle = mac_usernotifications::Notification::new()
        .title(title)
        .message(body)
        .send()
        .await
        .map_err(|error| format!("could not show the OS notification: {error}"))?;

    // Tauri drives the AppKit main run loop, so the modern notification center
    // can deliver response callbacks while this lightweight future waits. This
    // avoids the timing race in notify-rust's blocking compatibility waiter.
    tauri::async_runtime::spawn(async move {
        if handle
            .response()
            .await
            .is_ok_and(|response| response.is_default_action())
        {
            activate_notification(&app, notification_id);
        }
    });

    Ok(())
}

#[cfg(not(target_os = "macos"))]
async fn show_notification(
    app: AppHandle,
    notification_id: i64,
    title: String,
    body: String,
) -> Result<(), String> {
    let handle = tauri::async_runtime::spawn_blocking(move || {
        let mut notification = notify_rust::Notification::new();
        notification.summary(&title).body(&body).auto_icon();

        // Freedesktop notification servers require an explicit default action
        // before clicking the banner can be observed. Windows exposes the
        // banner's default activation without rendering an extra button.
        #[cfg(unix)]
        notification.action("default", "Open Workman");

        notification
            .show()
            .map_err(|error| format!("could not show the OS notification: {error}"))
    })
    .await
    .map_err(|error| format!("notification task failed: {error}"))??;

    // tauri-plugin-notification deliberately treats desktop notifications as
    // fire-and-forget. Retaining the same backend handle is the narrow seam that
    // lets Workman identify a banner click and route to the matching row.
    tauri::async_runtime::spawn_blocking(move || {
        handle.wait_for_action(|action| {
            if action == "default" {
                activate_notification(&app, notification_id);
            }
        });
    });

    Ok(())
}

fn activate_notification(app: &AppHandle, notification_id: i64) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.unminimize();
        let _ = window.show();
        let _ = window.set_focus();
    }
    let _ = app.emit(ACTION_EVENT, NotificationAction { notification_id });
}

fn checked_copy(label: &str, value: String, max_chars: usize) -> Result<String, String> {
    let value = value.trim();
    if value.is_empty() {
        return Err(format!("notification {label} cannot be empty"));
    }
    if value.chars().count() > max_chars {
        return Err(format!("notification {label} is too long"));
    }
    Ok(value.to_owned())
}

#[cfg(target_os = "macos")]
async fn permission_state() -> Result<NotificationPermission, String> {
    use mac_usernotifications::{
        AuthorizationStatus, NotificationSettingStatus, get_notification_settings,
    };

    let settings = get_notification_settings()
        .await
        .map_err(|error| format!("could not read macOS notification permission: {error}"))?;
    let (state, detail) = match settings.authorization_status {
        AuthorizationStatus::NotDetermined => (
            "not_determined",
            Some("macOS has not asked for notification permission yet.".to_owned()),
        ),
        AuthorizationStatus::Denied => (
            "denied",
            Some("Notifications are denied in macOS System Settings.".to_owned()),
        ),
        AuthorizationStatus::Authorized
            if settings.alert_enabled == NotificationSettingStatus::Disabled =>
        {
            (
                "denied",
                Some(
                    "Notification permission is allowed, but macOS banners are disabled in System Settings."
                        .to_owned(),
                ),
            )
        }
        AuthorizationStatus::Authorized
            if settings.alert_enabled == NotificationSettingStatus::NotSupported =>
        {
            (
                "unavailable",
                Some("This macOS account does not support notification banners.".to_owned()),
            )
        }
        AuthorizationStatus::Authorized
            if settings.alert_enabled == NotificationSettingStatus::Unknown =>
        {
            (
                "unknown",
                Some("macOS returned an unknown banner setting for Workman.".to_owned()),
            )
        }
        AuthorizationStatus::Authorized => (
            "granted",
            Some("Notifications are allowed by macOS.".to_owned()),
        ),
        AuthorizationStatus::Provisional => (
            "granted",
            Some("macOS delivers notifications quietly.".to_owned()),
        ),
        AuthorizationStatus::Ephemeral => (
            "granted",
            Some("macOS granted temporary notification permission.".to_owned()),
        ),
        AuthorizationStatus::Unknown => (
            "unknown",
            Some("macOS returned an unknown notification permission state.".to_owned()),
        ),
    };
    Ok(NotificationPermission {
        state,
        platform: "macos",
        detail,
    })
}

#[cfg(target_os = "macos")]
async fn request_permission() -> Result<NotificationPermission, String> {
    mac_usernotifications::request_auth()
        .await
        .map_err(|error| format!("could not request macOS notification permission: {error}"))?;
    permission_state().await
}

#[cfg(not(target_os = "macos"))]
async fn permission_state() -> Result<NotificationPermission, String> {
    Ok(NotificationPermission {
        state: "granted",
        platform: std::env::consts::OS,
        detail: Some(
            "Notifications are available through the desktop notification service.".into(),
        ),
    })
}

#[cfg(not(target_os = "macos"))]
async fn request_permission() -> Result<NotificationPermission, String> {
    permission_state().await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn notification_copy_rejects_empty_and_oversize_values() {
        assert!(checked_copy("title", "   ".into(), 10).is_err());
        assert!(checked_copy("body", "eleven chars".into(), 5).is_err());
        assert_eq!(
            checked_copy("title", "  Ready  ".into(), 10).unwrap(),
            "Ready"
        );
    }
}
