#[tokio::main]
async fn main() {
    if let Err(error) = awm::run_env().await {
        eprintln!("awm: {error}");
        std::process::exit(1);
    }
}
