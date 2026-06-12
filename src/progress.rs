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
    fn on_step(&self, step: Step, detail: Option<&str>);

    fn on_file_start(&self, _path: &Path) {}

    fn on_dry_run(&self, _title: &str, _md: &Path, _pdf: &Path) {}

    fn on_error(&self, _path: &Path, _error: &Error) {}
}

pub struct StderrProgress;

impl Progress for StderrProgress {
    fn on_step(&self, step: Step, detail: Option<&str>) {
        match detail {
            Some(d) => eprintln!("[{}/5] {} {d}...", step.number(), step.label()),
            None => eprintln!("[{}/5] {}...", step.number(), step.label()),
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
    fn on_step(&self, _step: Step, _detail: Option<&str>) {}
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A mock Progress that records (step, detail) tuples.
    struct RecordingProgress {
        calls: std::sync::Mutex<Vec<(Step, Option<String>)>>,
    }

    impl RecordingProgress {
        fn new() -> Self {
            Self {
                calls: std::sync::Mutex::new(Vec::new()),
            }
        }

        fn calls(&self) -> Vec<(Step, Option<String>)> {
            self.calls.lock().unwrap().clone()
        }
    }

    impl Progress for RecordingProgress {
        fn on_step(&self, step: Step, detail: Option<&str>) {
            self.calls
                .lock()
                .unwrap()
                .push((step, detail.map(|s| s.to_string())));
        }
    }

    #[test]
    fn on_step_with_some_detail_records_value() {
        let rec = RecordingProgress::new();
        rec.on_step(Step::Truncate, Some("file.pdf"));
        let calls = rec.calls();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].0, Step::Truncate);
        assert_eq!(calls[0].1.as_deref(), Some("file.pdf"));
    }

    #[test]
    fn on_step_with_none_records_none() {
        let rec = RecordingProgress::new();
        rec.on_step(Step::Ocr, None);
        let calls = rec.calls();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].0, Step::Ocr);
        assert_eq!(calls[0].1, None);
    }

    #[test]
    fn noop_progress_compiles_with_option() {
        let noop = NoopProgress;
        noop.on_step(Step::WriteOutputs, None);
        noop.on_step(Step::ExtractTitle, Some("detail"));
    }
}
