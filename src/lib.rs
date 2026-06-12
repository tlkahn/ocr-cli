//! Local-first OCR pipeline for academic PDFs.
//!
//! # Example
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
//! **Note:** `process_file` is `async` and performs blocking PDF I/O internally.
//! In Tauri or other GUI runtimes, call it from a background task (e.g.
//! `tokio::task::spawn_blocking`) to avoid blocking the main thread.

pub mod cli;
pub mod config;
pub mod error;
pub mod ocr;
pub mod pipeline;
pub mod postproc;
pub mod progress;
pub mod title;
pub mod truncate;
