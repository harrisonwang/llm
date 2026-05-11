#[tokio::main]
async fn main() {
    if let Err(err) = llm_cli::run_cli().await {
        eprintln!("error: {err:#}");
        std::process::exit(1);
    }
}
