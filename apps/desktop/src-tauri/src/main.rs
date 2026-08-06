#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    let args = std::env::args_os().collect::<Vec<_>>();
    if let Some(data_dir) = workman_desktop::embedded_daemon_data_dir(args.iter().cloned()) {
        workman_desktop::run_embedded_daemon(data_dir).expect("failed to run embedded workmand");
    } else {
        let (data_dir, daemon_bin) = workman_desktop::launch_environment(args);
        // SAFETY: launch overrides are installed before Tauri or Tokio starts worker threads.
        unsafe {
            if let Some(data_dir) = data_dir {
                std::env::set_var("WORKMAN_DATA_DIR", data_dir);
            }
            if let Some(daemon_bin) = daemon_bin {
                std::env::set_var("WORKMAN_DAEMON_BIN", daemon_bin);
            }
        }
        workman_desktop::run();
    }
}
