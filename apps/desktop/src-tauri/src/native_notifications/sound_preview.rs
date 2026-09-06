//! Explicit sound previews do not create notifications or change notification permissions.
use std::path::Path;

#[cfg(target_os = "macos")]
pub fn play(path: Option<&Path>) -> Result<(), String> {
    let path = path.ok_or("No sound file is selected.")?;
    run_player(std::process::Command::new("/usr/bin/afplay").arg(path))
        .map_err(|error| format!("Could not preview the notification sound: {error}"))
}

#[cfg(windows)]
pub fn play(_path: Option<&Path>) -> Result<(), String> {
    use windows::{
        Win32::Media::Audio::{PlaySoundW, SND_ALIAS, SND_SYNC, SND_SYSTEM},
        core::w,
    };
    // Match the Notification.Default event used by the native toast backend. No file path
    // comes from the webview; Windows builds currently support only the system sound.
    if unsafe {
        PlaySoundW(
            w!("Notification.Default"),
            None,
            SND_ALIAS | SND_SYNC | SND_SYSTEM,
        )
    }
    .as_bool()
    {
        Ok(())
    } else {
        Err("Could not preview the Windows notification sound. Check system sound settings.".into())
    }
}

#[cfg(all(unix, not(target_os = "macos")))]
pub fn play(path: Option<&Path>) -> Result<(), String> {
    let mut player = std::process::Command::new("canberra-gtk-play");
    match path {
        Some(path) => {
            player.arg("--file").arg(path);
        }
        None => {
            player.args(["--id", "message-new-instant"]);
        }
    }
    match run_player(&mut player) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            if let Some(path) = path {
                for program in ["paplay", "aplay"] {
                    match run_player(std::process::Command::new(program).arg(path)) {
                        Ok(()) => return Ok(()),
                        Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
                        Err(error) => return Err(format!("Could not preview the sound: {error}")),
                    }
                }
            }
            Err("Sound preview needs canberra-gtk-play (or paplay/aplay for a saved WAV). Install a player using your Linux distribution's package manager.".into())
        }
        Err(error) => Err(format!("Could not preview the notification sound: {error}")),
    }
}

#[cfg(unix)]
fn run_player(command: &mut std::process::Command) -> std::io::Result<()> {
    use std::{
        process::Stdio,
        time::{Duration, Instant},
    };
    let mut child = command
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()?;
    let deadline = Instant::now() + Duration::from_secs(31);
    let result = loop {
        match child.try_wait() {
            Ok(Some(status)) if status.success() => break Ok(()),
            Ok(Some(status)) => {
                break Err(std::io::Error::other(format!(
                    "audio player exited with {status}"
                )));
            }
            Err(error) => break Err(error),
            Ok(None) if Instant::now() >= deadline => {
                break Err(std::io::Error::new(
                    std::io::ErrorKind::TimedOut,
                    "audio player did not finish",
                ));
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(50)),
        }
    };
    if result.is_err() {
        let _ = child.kill();
        let _ = child.wait();
    }
    result
}
