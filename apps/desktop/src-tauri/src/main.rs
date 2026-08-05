#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    if let Some(data_dir) = awm_desktop::embedded_daemon_data_dir(std::env::args_os()) {
        awm_desktop::run_embedded_daemon(data_dir).expect("failed to run embedded awmd");
    } else {
        awm_desktop::run();
    }
}
