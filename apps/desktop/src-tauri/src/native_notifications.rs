use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager};

pub const ACTION_EVENT: &str = "notification://action";

#[cfg(any(windows, test))]
mod badge;
#[cfg(all(unix, not(target_os = "macos")))]
mod linux;
#[cfg(windows)]
#[path = "native_notifications/windows.rs"]
mod windows_backend;

#[derive(Default)]
pub struct NativeNotificationState {
    #[cfg(all(unix, not(target_os = "macos")))]
    linux: linux::Backend,
    #[cfg(windows)]
    windows: windows_backend::Backend,
}

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
pub async fn native_notification_permission_state(
    app: AppHandle,
) -> Result<NotificationPermission, String> {
    permission_state(&app).await
}

#[tauri::command]
pub async fn native_notification_request_permission(
    app: AppHandle,
) -> Result<NotificationPermission, String> {
    request_permission(&app).await
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

#[tauri::command]
pub async fn native_notification_dismiss(
    _app: AppHandle,
    notification_ids: Vec<i64>,
) -> Result<(), String> {
    if notification_ids.len() > 1_000 || notification_ids.iter().any(|id| *id <= 0) {
        return Err("invalid notification IDs".into());
    }
    #[cfg(target_os = "macos")]
    for id in notification_ids {
        let identifier = notification_identifier(id);
        mac_usernotifications::cancel_pending(&identifier).await;
        mac_usernotifications::close_delivered(&identifier).await;
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    _app.state::<NativeNotificationState>()
        .linux
        .dismiss(&notification_ids)
        .await?;
    #[cfg(windows)]
    {
        let backend = _app.state::<NativeNotificationState>().windows.clone();
        let app_id = _app.config().identifier.clone();
        tauri::async_runtime::spawn_blocking(move || backend.dismiss(&app_id, &notification_ids))
            .await
            .map_err(|error| error.to_string())??;
    }
    Ok(())
}

#[tauri::command]
pub fn native_notification_set_badge(app: AppHandle, count: u32) -> Result<(), String> {
    let window = app
        .get_webview_window("main")
        .ok_or("main window is unavailable")?;
    #[cfg(windows)]
    return window
        .set_overlay_icon(
            (count > 0).then(|| tauri::image::Image::new_owned(badge::pixels(count), 32, 32)),
        )
        .map_err(|error| error.to_string());
    #[cfg(not(windows))]
    window
        .set_badge_count((count > 0).then_some(i64::from(count)))
        .map_err(|error| error.to_string())
}

#[cfg(target_os = "macos")]
fn notification_identifier(id: i64) -> String {
    format!("workman.notification.{id}")
}

#[cfg(target_os = "macos")]
async fn show_notification(
    app: AppHandle,
    notification_id: i64,
    title: String,
    body: String,
) -> Result<(), String> {
    let mut notification = mac_usernotifications::Notification::new()
        .title(title)
        .message(body);
    if notification_id > 0 {
        // The durable notification ID lets reading an agent clear the matching Notification
        // Center entry, including notifications delivered before the desktop was restarted.
        notification = notification.id(&notification_identifier(notification_id));
    }
    let handle = notification
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

#[cfg(all(unix, not(target_os = "macos")))]
async fn show_notification(
    app: AppHandle,
    notification_id: i64,
    title: String,
    body: String,
) -> Result<(), String> {
    let backend = app.state::<NativeNotificationState>().linux.clone();
    backend
        .show(notification_id, &title, &body, move || {
            activate_notification(&app, notification_id)
        })
        .await
}

#[cfg(windows)]
async fn show_notification(
    app: AppHandle,
    notification_id: i64,
    title: String,
    body: String,
) -> Result<(), String> {
    let backend = app.state::<NativeNotificationState>().windows.clone();
    let app_id = app.config().identifier.clone();
    let name = app
        .config()
        .product_name
        .clone()
        .unwrap_or_else(|| "Workman".into());
    let executable = std::env::current_exe().map_err(|error| error.to_string())?;
    tauri::async_runtime::spawn_blocking(move || {
        backend.prepare(&app_id, &name, &executable)?;
        backend.show(&app_id, notification_id, &title, &body, move || {
            activate_notification(&app, notification_id)
        })
    })
    .await
    .map_err(|error| error.to_string())?
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
async fn permission_state(_app: &AppHandle) -> Result<NotificationPermission, String> {
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
async fn request_permission(app: &AppHandle) -> Result<NotificationPermission, String> {
    mac_usernotifications::request_auth()
        .await
        .map_err(|error| format!("could not request macOS notification permission: {error}"))?;
    permission_state(app).await
}

#[cfg(all(unix, not(target_os = "macos")))]
async fn permission_state(app: &AppHandle) -> Result<NotificationPermission, String> {
    let capabilities = app
        .state::<NativeNotificationState>()
        .linux
        .capabilities()
        .await;
    Ok(NotificationPermission {
        state: if capabilities.is_ok() { "granted" } else { "unavailable" },
        platform: std::env::consts::OS,
        detail: Some(match capabilities {
            Ok(capabilities) if capabilities.iter().any(|capability| capability == "actions") => "Desktop notifications are available. Banner history and app icon badges depend on your desktop environment.".into(),
            Ok(_) => "This desktop can show notifications but does not support clicking them to open Workman.".into(),
            Err(error) => format!("No desktop notification service is available: {error}"),
        }),
    })
}

#[cfg(windows)]
async fn permission_state(app: &AppHandle) -> Result<NotificationPermission, String> {
    let backend = app.state::<NativeNotificationState>().windows.clone();
    let app_id = app.config().identifier.clone();
    let name = app
        .config()
        .product_name
        .clone()
        .unwrap_or_else(|| "Workman".into());
    let executable = std::env::current_exe().map_err(|error| error.to_string())?;
    let allowed = tauri::async_runtime::spawn_blocking(move || {
        backend.prepare(&app_id, &name, &executable)?;
        backend.permission(&app_id)
    })
    .await
    .map_err(|error| error.to_string())??;
    Ok(NotificationPermission {
        state: if allowed { "granted" } else { "denied" },
        platform: "windows",
        detail: Some(
            if allowed {
                "Windows notifications are allowed. Do Not Disturb may deliver them quietly."
            } else {
                "Notifications are disabled in Windows Settings or by your organization's policy."
            }
            .into(),
        ),
    })
}

#[cfg(not(target_os = "macos"))]
async fn request_permission(app: &AppHandle) -> Result<NotificationPermission, String> {
    permission_state(app).await
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
