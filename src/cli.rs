use clap::Parser;
use std::path::PathBuf;

/// Local-first OCR pipeline: truncate, extract title, OCR via Mistral, post-process, archive.
#[derive(Debug, Parser)]
#[command(name = "ocr-cli", version, about)]
pub struct Cli {
    /// PDF files to process
    #[arg(required = true)]
    pub files: Vec<PathBuf>,

    /// Number of leading pages to remove
    #[arg(long, default_value_t = 0)]
    pub lead: usize,

    /// Number of trailing pages to remove
    #[arg(long, default_value_t = 0)]
    pub trail: usize,

    /// Path to the Obsidian vault output directory
    #[arg(long, default_value = "~/Documents/Ekuro/")]
    pub vault: PathBuf,

    /// Path to archive processed PDFs
    #[arg(long, default_value = "~/Documents/Papers/")]
    pub papers: PathBuf,

    /// LLM model for title extraction
    #[arg(long, default_value = "gpt-4o-mini")]
    pub model: String,

    /// Show proposed actions without executing
    #[arg(long, default_value_t = false)]
    pub dry_run: bool,

    /// Enable verbose output
    #[arg(long, default_value_t = false)]
    pub verbose: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_defaults_single_file() {
        let cli = Cli::try_parse_from(["ocr-cli", "paper.pdf"]).unwrap();
        assert_eq!(cli.files, vec![PathBuf::from("paper.pdf")]);
        assert_eq!(cli.lead, 0);
        assert_eq!(cli.trail, 0);
        assert_eq!(cli.vault, PathBuf::from("~/Documents/Ekuro/"));
        assert_eq!(cli.papers, PathBuf::from("~/Documents/Papers/"));
        assert_eq!(cli.model, "gpt-4o-mini");
        assert!(!cli.dry_run);
        assert!(!cli.verbose);
    }

    #[test]
    fn test_lead_trail_override() {
        let cli =
            Cli::try_parse_from(["ocr-cli", "--lead", "2", "--trail", "3", "paper.pdf"]).unwrap();
        assert_eq!(cli.lead, 2);
        assert_eq!(cli.trail, 3);
        // Other defaults unaffected
        assert_eq!(cli.files, vec![PathBuf::from("paper.pdf")]);
        assert_eq!(cli.vault, PathBuf::from("~/Documents/Ekuro/"));
        assert_eq!(cli.papers, PathBuf::from("~/Documents/Papers/"));
        assert_eq!(cli.model, "gpt-4o-mini");
        assert!(!cli.dry_run);
        assert!(!cli.verbose);
    }

    #[test]
    fn test_dry_run_flag() {
        let cli = Cli::try_parse_from(["ocr-cli", "--dry-run", "paper.pdf"]).unwrap();
        assert!(cli.dry_run);
        // Without the flag, dry_run is false (already covered by test_defaults_single_file)
    }

    #[test]
    fn test_multiple_files() {
        let cli = Cli::try_parse_from(["ocr-cli", "a.pdf", "b.pdf", "c.pdf"]).unwrap();
        assert_eq!(cli.files.len(), 3);
        assert_eq!(
            cli.files,
            vec![
                PathBuf::from("a.pdf"),
                PathBuf::from("b.pdf"),
                PathBuf::from("c.pdf"),
            ]
        );
    }

    #[test]
    fn test_missing_files_fails() {
        let result = Cli::try_parse_from(["ocr-cli"]);
        assert!(result.is_err());
    }
}
