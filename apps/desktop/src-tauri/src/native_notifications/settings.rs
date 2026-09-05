//! Fixed destinations for notification permission recovery; no caller-supplied URLs or commands.
use std::process::{Command, Stdio};

#[cfg(target_os = "macos")]
pub fn open(app_id: &str) -> Result<(), String> {
    let mut url =
        url::Url::parse("x-apple.systempreferences:com.apple.Notifications-Settings.extension")
            .map_err(|error| error.to_string())?;
    url.query_pairs_mut().append_pair("id", app_id);
    run_launcher(Command::new("/usr/bin/open").arg(url.as_str()))
}

#[cfg(windows)]
pub fn open(_app_id: &str) -> Result<(), String> {
    use std::os::windows::process::CommandExt;
    // Windows documents this URI for Notifications & actions. /d disables cmd AutoRun;
    // all arguments are fixed, and the empty title is required by `start`.
    run_launcher(
        Command::new("cmd")
            .args(["/d", "/c", "start", "", "ms-settings:notifications"])
            .creation_flags(0x08000000), // CREATE_NO_WINDOW
    )
}

#[cfg(any(target_os = "macos", windows))]
fn run_launcher(command: &mut Command) -> Result<(), String> {
    let status = command
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map_err(|error| format!("Could not open system notification settings: {error}"))?;
    if status.success() {
        Ok(())
    } else {
        Err(format!(
            "Could not open system notification settings ({status})."
        ))
    }
}

#[cfg(all(unix, not(target_os = "macos")))]
pub fn open(_app_id: &str) -> Result<(), String> {
    let desktop = std::env::var("XDG_CURRENT_DESKTOP")
        .ok()
        .filter(|desktop| !desktop.is_empty())
        .or_else(|| std::env::var("XDG_SESSION_DESKTOP").ok())
        .unwrap_or_default()
        .to_ascii_lowercase();
    let desktops: Vec<_> = desktop.split(':').collect();
    let candidates: &[(&str, &[&str])] = if desktops.contains(&"kde")
        || desktops.contains(&"plasma")
    {
        &[
            ("systemsettings", &["kcm_notifications"]),
            ("systemsettings5", &["kcm_notifications"]),
        ]
    } else if desktops.contains(&"gnome") || desktops.contains(&"unity") {
        &[("gnome-control-center", &["notifications"])]
    } else if desktops.contains(&"xfce") {
        &[("xfce4-notifyd-config", &[])]
    } else {
        return Err("Open your desktop's Settings app and choose Notifications. A direct shortcut is not available for this desktop environment.".into());
    };
    for (program, arguments) in candidates {
        match Command::new(program)
            .args(*arguments)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
        {
            Ok(mut child) => {
                // Settings may remain open for the entire session. Reap it without holding the UI busy.
                std::thread::spawn(move || {
                    let _ = child.wait();
                });
                return Ok(());
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
            Err(error) => {
                return Err(format!(
                    "Could not open system notification settings: {error}"
                ));
            }
        }
    }
    Err("The desktop notification settings app could not be found. Open your desktop's Settings app and choose Notifications.".into())
}
