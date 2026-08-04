#[tokio::main]
async fn main() {
    if let Err(error) = gbuild_cli::run_env().await {
        eprintln!("gbuild: {error}");
        std::process::exit(1);
    }
}
