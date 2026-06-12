use std::path::Path;

use crate::error::Error;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Step {
    Truncate,
    ExtractTitle,
    Ocr,
    PostProcess,
    WriteOutputs,
}

impl Step {
    pub fn number(&self) -> u8 {
        match self {
            Step::Truncate => 1,
            Step::ExtractTitle => 2,
            Step::Ocr => 3,
            Step::PostProcess => 4,
            Step::WriteOutputs => 5,
        }
    }

    pub fn label(&self) -> &'static str {
        match self {
            Step::Truncate => "Truncating",
            Step::ExtractTitle => "Extracting title",
            Step::Ocr => "Running OCR",
            Step::PostProcess => "Post-processing",
            Step::WriteOutputs => "Writing outputs",
        }
    }
}

pub trait Progress: Send + Sync {
    fn on_step(&self, step: Step, detail: &str);

    fn on_file_start(&self, _path: &Path) {}

    fn on_dry_run(&self, _title: &str, _md: &Path, _pdf: &Path) {}

    fn on_error(&self, _path: &Path, _error: &Error) {}
}

pub struct StderrProgress;

impl Progress for StderrProgress {
    fn on_step(&self, step: Step, detail: &str) {
        if detail.is_empty() {
            eprintln!("[{}/5] {}...", step.number(), step.label());
        } else {
            eprintln!("[{}/5] {} {detail}...", step.number(), step.label());
        }
    }

    fn on_file_start(&self, path: &Path) {
        eprintln!("\n=== Processing: {} ===", path.display());
    }

    fn on_dry_run(&self, title: &str, md: &Path, pdf: &Path) {
        eprintln!("[dry-run] Proposed filename: {title}");
        eprintln!("[dry-run]   markdown: {}", md.display());
        eprintln!("[dry-run]   pdf:      {}", pdf.display());
    }

    fn on_error(&self, path: &Path, error: &Error) {
        eprintln!("ERROR processing {}: {error}", path.display());
    }
}

pub struct NoopProgress;

impl Progress for NoopProgress {
    fn on_step(&self, _step: Step, _detail: &str) {}
}
