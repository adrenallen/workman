#[tokio::main]
async fn main() {
    if let Err(error) = workman_cli::run_env().await {
        eprintln!("wrk: {error}");
        std::process::exit(1);
    }
}
