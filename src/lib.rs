//! Local-first OCR pipeline for academic PDFs.
//!
//! # Example (builder -- programmatic keys)
//!
//! ```rust,no_run
//! use ocr_cli::config::Config;
//! use ocr_cli::pipeline::{Options, process_file};
//! use ocr_cli::progress::NoopProgress;
//!
//! # async fn run() -> ocr_cli::error::Result<()> {
//! let config = Config::builder("sk-mistral-...", "sk-openai-...")
//!     .vault_path("/path/to/vault")
//!     .papers_path("/path/to/papers")
//!     .build()?;
//! let client = reqwest::Client::new();
//! let result = process_file(
//!     std::path::Path::new("paper.pdf"),
//!     &Options::default(),
//!     &config,
//!     &client,
//!     None,
//!     &NoopProgress,
//! ).await?;
//! # Ok(())
//! # }
//! ```
//!
//! # Example (from environment)
//!
//! ```rust,no_run
//! use ocr_cli::config::{Config, ConfigOverrides};
//! use ocr_cli::pipeline::{Options, process_file};
//! use ocr_cli::progress::NoopProgress;
//!
//! # async fn run() -> ocr_cli::error::Result<()> {
//! let config = Config::from_env(&ConfigOverrides::default())?;
//! let client = reqwest::Client::new();
//! let result = process_file(
//!     std::path::Path::new("paper.pdf"),
//!     &Options::default(),
//!     &config,
//!     &client,
//!     None,
//!     &NoopProgress,
//! ).await?;
//! # Ok(())
//! # }
//! ```
//!
//! **Note:** `process_file` is an `async fn` that internally awaits network
//! calls (title extraction via OpenAI, OCR via Mistral) and performs some
//! synchronous pdfium + `std::fs` work. Drive it on the async runtime --
//! e.g. `tokio::spawn` or `tauri::async_runtime::spawn` -- rather than
//! `tokio::task::spawn_blocking` (which takes a sync closure and cannot
//! `.await`). If the synchronous pdfium steps prove costly, offload only
//! those via `spawn_blocking` inside the future.

#[doc(hidden)]
pub mod cli;
pub mod config;
pub mod error;
pub(crate) mod ocr;
pub mod pipeline;
pub(crate) mod postproc;
pub mod progress;
pub(crate) mod title;
pub(crate) mod truncate;
