mod cli;
mod config;
mod error;
mod ocr;
mod pipeline;
mod postproc;
mod title;
mod truncate;

use clap::Parser;
use cli::Cli;
use config::Config;

#[tokio::main]
async fn main() {
    let cli = Cli::parse();
    let config = match Config::resolve(&cli) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("Configuration error: {e}");
            std::process::exit(1);
        }
    };

    let results = pipeline::process_batch(&cli, &config).await;

    let total = results.len();
    let fail_count = results.iter().filter(|(_, r)| r.is_err()).count();

    if fail_count > 0 {
        eprintln!("\n{fail_count}/{total} file(s) failed.");
        std::process::exit(1);
    } else {
        eprintln!("\nAll {total} file(s) processed successfully.");
    }
}
