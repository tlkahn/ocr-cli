use clap::Parser;
use ocr_cli::cli::Cli;
use ocr_cli::config::Config;

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

    let results = ocr_cli::pipeline::process_batch(&cli, &config).await;

    let total = results.len();
    let fail_count = results.iter().filter(|(_, r)| r.is_err()).count();

    if fail_count > 0 {
        eprintln!("\n{fail_count}/{total} file(s) failed.");
        std::process::exit(1);
    } else {
        eprintln!("\nAll {total} file(s) processed successfully.");
    }
}
