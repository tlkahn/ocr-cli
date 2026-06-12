use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use crate::cli::Cli;
use crate::config::Config;
use crate::error::Result;
use crate::progress::{Progress, StderrProgress, Step};

/// Clap-free options for driving the pipeline from library code.
#[derive(Debug, Clone, Default)]
pub struct Options {
    pub lead: usize,
    pub trail: usize,
    pub dry_run: bool,
}

impl From<&Cli> for Options {
    fn from(cli: &Cli) -> Self {
        Options {
            lead: cli.lead,
            trail: cli.trail,
            dry_run: cli.dry_run,
        }
    }
}

/// Outcome of successfully processing a single file.
#[derive(Debug)]
pub struct ProcessResult {
    /// The sanitized title extracted from the PDF.
    pub title: String,
    /// The path to the output markdown file in the vault.
    pub markdown_path: PathBuf,
    /// The path to the archived PDF in the papers directory.
    pub pdf_path: PathBuf,
    /// The path to the image assets directory (if any images were saved).
    pub images_dir: Option<PathBuf>,
}

/// Outcome of processing a single file through the pipeline.
#[derive(Debug)]
pub enum ProcessOutcome {
    /// dry_run=true: OCR was skipped; only the deduplicated title and
    /// proposed output paths are returned.
    DryRun {
        title: String,
        md_path: PathBuf,
        pdf_path: PathBuf,
    },
    /// Normal processing completed; full result available.
    Written(ProcessResult),
}

/// Build output paths from a title and config directories.
///
/// Returns (markdown_path, pdf_path, images_dir):
///   - markdown_path: vault/{title}.md
///   - pdf_path: papers/{title}.pdf
///   - images_dir: vault/assets/images/{title}/
pub fn output_paths(title: &str, vault: &Path, papers: &Path) -> (PathBuf, PathBuf, PathBuf) {
    let md = vault.join(format!("{title}.md"));
    let pdf = papers.join(format!("{title}.pdf"));
    let images = vault.join("assets").join("images").join(title);
    (md, pdf, images)
}

/// Wraps either a borrowed `&Pdfium` (caller-owned) or a lazily-initialised
/// `OnceLock<Pdfium>` so that both library and CLI callers can share one instance.
pub(crate) enum PdfiumHandle<'a> {
    Borrowed(&'a lmpdf::Pdfium),
    Lazy(OnceLock<lmpdf::Pdfium>),
}

impl PdfiumHandle<'_> {
    pub(crate) fn get_or_init(&self, pdfium_path: &Path) -> Result<&lmpdf::Pdfium> {
        match *self {
            PdfiumHandle::Borrowed(p) => Ok(p),
            PdfiumHandle::Lazy(ref lock) => {
                if lock.get().is_none() {
                    let p = lmpdf::Pdfium::open(pdfium_path)?;
                    let _ = lock.set(p);
                }
                Ok(lock.get().expect("pdfium initialized above"))
            }
        }
    }
}

/// Trait for extracting page text from PDF bytes (testability seam).
pub(crate) trait PageTextFn {
    fn page_text(
        &self,
        pdf_bytes: &[u8],
        pdfium: &PdfiumHandle<'_>,
        pdfium_path: &Path,
    ) -> Result<String>;
}

/// Default production implementation using pdfium.
struct DefaultPageText;

impl PageTextFn for DefaultPageText {
    fn page_text(
        &self,
        pdf_bytes: &[u8],
        pdfium: &PdfiumHandle<'_>,
        pdfium_path: &Path,
    ) -> Result<String> {
        let pdfium = pdfium.get_or_init(pdfium_path)?;
        let doc = pdfium.load_document(pdf_bytes, None)?;
        doc.page_text(0).map_err(Into::into)
    }
}

/// Testable inner implementation with injectable page_text trait and base URLs from config.
pub(crate) async fn process_file_inner(
    input: &Path,
    options: &Options,
    config: &Config,
    client: &reqwest::Client,
    page_text_fn: &dyn PageTextFn,
    pdfium: &PdfiumHandle<'_>,
    progress: &dyn Progress,
) -> Result<ProcessOutcome> {
    let filename = input.file_name().unwrap_or_default().to_string_lossy();

    // Step 1: Truncate
    progress.on_step(Step::Truncate, Some(&filename));
    let pdf_bytes = crate::truncate::truncate_pdf(
        input,
        options.lead,
        options.trail,
        pdfium,
        &config.pdfium_path,
    )?;

    // Step 2: Extract title
    progress.on_step(Step::ExtractTitle, None);
    let page_text = page_text_fn.page_text(&pdf_bytes, pdfium, &config.pdfium_path)?;
    let title = crate::title::extract_title(
        &page_text,
        &config.model,
        &config.openai_api_key,
        &config.openai_base_url,
    )
    .await?;

    if options.dry_run {
        let title = deduplicate_title(&title, &config.vault_path, &config.papers_path);
        let (md, pdf, _img) = output_paths(&title, &config.vault_path, &config.papers_path);
        progress.on_dry_run(&title, &md, &pdf);
        return Ok(ProcessOutcome::DryRun {
            title,
            md_path: md,
            pdf_path: pdf,
        });
    }

    // Step 3: Mistral OCR
    progress.on_step(Step::Ocr, None);
    let ocr_response = crate::ocr::ocr_pdf(
        client,
        &config.mistral_base_url,
        &config.mistral_api_key,
        &pdf_bytes,
        true,
    )
    .await?;

    // Step 4: Post-process
    progress.on_step(Step::PostProcess, None);
    let title = deduplicate_title(&title, &config.vault_path, &config.papers_path);
    let (md_path, pdf_path, images_dir) =
        output_paths(&title, &config.vault_path, &config.papers_path);
    let output = crate::postproc::postprocess(&ocr_response.pages, &images_dir, &title)?;

    // Step 5: Move outputs
    progress.on_step(Step::WriteOutputs, None);
    if let Some(parent) = md_path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    if md_path.exists() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::AlreadyExists,
            format!("markdown file already exists: {}", md_path.display()),
        )
        .into());
    }
    std::fs::write(&md_path, &output.markdown)?;

    if let Some(parent) = pdf_path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    move_file(input, &pdf_path)?;

    let has_images = !output.saved_images.is_empty();

    Ok(ProcessOutcome::Written(ProcessResult {
        title,
        markdown_path: md_path,
        pdf_path,
        images_dir: if has_images { Some(images_dir) } else { None },
    }))
}

/// Process a single PDF file through the full pipeline.
///
/// # Pdfium lifetime
///
/// Passing `None` for `pdfium` loads the pdfium dynamic library on **every
/// call** (a fresh [`OnceLock`] is created inside this function each time).
/// For callers processing multiple files this is wasteful -- open the library
/// once and reuse it:
///
/// ```rust,no_run
/// # use ocr_cli::config::{Config, ConfigOverrides};
/// # use ocr_cli::pipeline::{Options, process_file};
/// # use ocr_cli::progress::NoopProgress;
/// # async fn run() -> ocr_cli::error::Result<()> {
/// let config = Config::from_env(&ConfigOverrides::default())?;
/// let client = reqwest::Client::new();
/// let pdfium = lmpdf::Pdfium::open(&config.pdfium_path)?;
///
/// for path in &["a.pdf", "b.pdf"] {
///     process_file(
///         std::path::Path::new(path),
///         &Options::default(),
///         &config,
///         &client,
///         Some(&pdfium),
///         &NoopProgress,
///     ).await?;
/// }
/// # Ok(())
/// # }
/// ```
///
/// Alternatively, use [`process_batch`] which shares a single pdfium handle
/// across all files internally.
pub async fn process_file(
    input: &Path,
    options: &Options,
    config: &Config,
    client: &reqwest::Client,
    pdfium: Option<&lmpdf::Pdfium>,
    progress: &dyn Progress,
) -> Result<ProcessOutcome> {
    let handle = match pdfium {
        Some(p) => PdfiumHandle::Borrowed(p),
        None => PdfiumHandle::Lazy(OnceLock::new()),
    };
    process_file_inner(
        input,
        options,
        config,
        client,
        &DefaultPageText,
        &handle,
        progress,
    )
    .await
}

/// Process all files in batch mode. Continues past individual failures.
/// Returns a vec of (path, result) pairs.
pub async fn process_batch(cli: &Cli, config: &Config) -> Vec<(PathBuf, Result<ProcessOutcome>)> {
    let options = Options::from(cli);
    process_batch_inner(
        &cli.files,
        &options,
        config,
        &DefaultPageText,
        &StderrProgress,
    )
    .await
}

/// Testable batch processing with injectable page_text trait.
///
/// A single `OnceLock<lmpdf::Pdfium>` is created here and shared across
/// all files in the batch, so the library is loaded at most once.
pub(crate) async fn process_batch_inner(
    files: &[PathBuf],
    options: &Options,
    config: &Config,
    page_text_fn: &dyn PageTextFn,
    progress: &dyn Progress,
) -> Vec<(PathBuf, Result<ProcessOutcome>)> {
    let client = reqwest::Client::new();
    let handle = PdfiumHandle::Lazy(OnceLock::new());
    let mut results = Vec::new();
    for path in files {
        progress.on_file_start(path);
        let result = process_file_inner(
            path,
            options,
            config,
            &client,
            page_text_fn,
            &handle,
            progress,
        )
        .await;
        if let Err(ref e) = result {
            progress.on_error(path, e);
        }
        results.push((path.clone(), result));
    }
    results
}

/// If `vault/{title}.md`, `papers/{title}.pdf`, or `vault/assets/images/{title}/`
/// already exists, append a numeric suffix (`-2`, `-3`, ...) until all three
/// slots are free. Returns the (possibly suffixed) title.
fn deduplicate_title(title: &str, vault: &Path, papers: &Path) -> String {
    let (md, pdf, images) = output_paths(title, vault, papers);
    if !md.exists() && !pdf.exists() && !images.exists() {
        return title.to_string();
    }
    let mut n = 2u32;
    loop {
        let candidate = format!("{title}-{n}");
        let (md, pdf, images) = output_paths(&candidate, vault, papers);
        if !md.exists() && !pdf.exists() && !images.exists() {
            return candidate;
        }
        n += 1;
    }
}

/// Move a file from `src` to `dst`, trying `fs::rename` first and falling back
/// to `fs::copy` + `fs::remove_file` for cross-device moves.
///
/// Returns `Err(Error::Io(AlreadyExists))` if `dst` already exists (no-clobber).
fn move_file(src: &Path, dst: &Path) -> Result<()> {
    if dst.exists() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::AlreadyExists,
            format!("destination already exists: {}", dst.display()),
        )
        .into());
    }
    if let Some(parent) = dst.parent() {
        std::fs::create_dir_all(parent)?;
    }
    match std::fs::rename(src, dst) {
        Ok(()) => Ok(()),
        Err(_) => {
            std::fs::copy(src, dst)?;
            std::fs::remove_file(src)?;
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::progress::NoopProgress;

    #[test]
    fn test_move_file_same_device() {
        let dir = tempfile::TempDir::new().unwrap();
        let src = dir.path().join("source.pdf");
        let dst = dir.path().join("dest.pdf");
        std::fs::write(&src, b"hello pdf").unwrap();

        move_file(&src, &dst).unwrap();

        assert!(!src.exists(), "source should be removed");
        assert!(dst.exists(), "destination should exist");
        assert_eq!(std::fs::read(&dst).unwrap(), b"hello pdf");
    }

    #[test]
    fn test_move_file_creates_parent_dirs() {
        let dir = tempfile::TempDir::new().unwrap();
        let src = dir.path().join("source.pdf");
        let dst = dir.path().join("a").join("b").join("c").join("dest.pdf");
        std::fs::write(&src, b"nested test").unwrap();

        move_file(&src, &dst).unwrap();

        assert!(!src.exists(), "source should be removed");
        assert!(dst.exists(), "destination should exist");
        assert_eq!(std::fs::read(&dst).unwrap(), b"nested test");
    }

    #[test]
    fn test_move_file_source_not_found() {
        let dir = tempfile::TempDir::new().unwrap();
        let src = dir.path().join("nonexistent.pdf");
        let dst = dir.path().join("dest.pdf");

        let result = move_file(&src, &dst);
        assert!(result.is_err(), "move_file should fail for missing source");
        let err = result.unwrap_err();
        assert!(
            matches!(err, crate::error::Error::Io(_)),
            "expected Error::Io, got: {err:?}"
        );
    }

    #[test]
    fn test_move_file_refuses_overwrite() {
        let dir = tempfile::TempDir::new().unwrap();
        let src = dir.path().join("source.pdf");
        let dst = dir.path().join("dest.pdf");
        std::fs::write(&src, b"new content").unwrap();
        std::fs::write(&dst, b"original content").unwrap();

        let result = move_file(&src, &dst);
        assert!(result.is_err(), "move_file should refuse to overwrite");
        let err = result.unwrap_err();
        assert!(
            matches!(err, crate::error::Error::Io(_)),
            "expected Error::Io, got: {err:?}"
        );

        // Destination content should be unchanged.
        assert_eq!(
            std::fs::read(&dst).unwrap(),
            b"original content",
            "destination should not be modified"
        );
        // Source should still exist (move was refused).
        assert!(src.exists(), "source should still exist after refused move");
    }

    struct MockPageText;

    impl PageTextFn for MockPageText {
        fn page_text(
            &self,
            _pdf_bytes: &[u8],
            _pdfium: &PdfiumHandle<'_>,
            _pdfium_path: &Path,
        ) -> crate::error::Result<String> {
            Ok("fake page text for title extraction".into())
        }
    }

    #[tokio::test]
    async fn test_dry_run_stops_after_title() {
        use wiremock::matchers::{method, path};
        use wiremock::{Mock, MockServer, ResponseTemplate};

        let mock_server = MockServer::start().await;

        // Mock the LLM title extraction endpoint.
        let title_body = serde_json::json!({
            "id": "chatcmpl-test",
            "object": "chat.completion",
            "model": "gpt-4o-mini",
            "choices": [{
                "index": 0,
                "message": {
                    "role": "assistant",
                    "content": "test-title-from-llm-smith-2024"
                },
                "finish_reason": "stop"
            }],
            "usage": {
                "prompt_tokens": 10,
                "completion_tokens": 5,
                "total_tokens": 15
            }
        });

        Mock::given(method("POST"))
            .and(path("/v1/chat/completions"))
            .respond_with(
                ResponseTemplate::new(200)
                    .insert_header("Content-Type", "application/json")
                    .set_body_json(&title_body),
            )
            .expect(1)
            .mount(&mock_server)
            .await;

        // Mock the OCR endpoint -- expect 0 calls (dry-run should skip OCR).
        Mock::given(method("POST"))
            .and(path("/v1/ocr"))
            .respond_with(ResponseTemplate::new(200))
            .expect(0)
            .mount(&mock_server)
            .await;

        // Set up temp dirs.
        let tmp = tempfile::TempDir::new().unwrap();
        let input_pdf = tmp.path().join("test.pdf");
        std::fs::write(&input_pdf, b"%PDF-fake").unwrap();
        let vault = tmp.path().join("vault");
        let papers = tmp.path().join("papers");

        let options = Options {
            lead: 0,
            trail: 0,
            dry_run: true,
        };
        let config = crate::config::Config {
            mistral_api_key: "sk-mistral-test".into(),
            openai_api_key: "sk-openai-test".into(),
            model: "gpt-4o-mini".into(),
            vault_path: vault,
            papers_path: papers,
            pdfium_path: PathBuf::from("/nonexistent/libpdfium.dylib"),
            openai_base_url: mock_server.uri(),
            mistral_base_url: mock_server.uri(),
        };
        let client = reqwest::Client::new();

        let handle = PdfiumHandle::Lazy(OnceLock::new());
        let result = process_file_inner(
            &input_pdf,
            &options,
            &config,
            &client,
            &MockPageText,
            &handle,
            &NoopProgress,
        )
        .await;

        assert!(
            result.is_ok(),
            "process_file_inner should succeed: {result:?}"
        );
        let outcome = result.unwrap();
        match outcome {
            ProcessOutcome::DryRun {
                title,
                md_path,
                pdf_path,
            } => {
                assert_eq!(title, "test-title-from-llm-smith-2024");
                assert!(
                    md_path
                        .to_string_lossy()
                        .contains("test-title-from-llm-smith-2024.md")
                );
                assert!(
                    pdf_path
                        .to_string_lossy()
                        .contains("test-title-from-llm-smith-2024.pdf")
                );
            }
            ProcessOutcome::Written(_) => panic!("expected DryRun, got Written"),
        }
        // wiremock will verify OCR endpoint was never called on drop.
    }

    /// Generate a minimal 1x1 red JPEG as base64 for testing.
    fn make_test_jpeg_base64() -> String {
        use base64::Engine as _;
        use image::{DynamicImage, Rgba};
        use std::io::Cursor;
        let mut img = DynamicImage::new_rgba8(1, 1);
        img.as_mut_rgba8()
            .unwrap()
            .put_pixel(0, 0, Rgba([255, 0, 0, 255]));
        let mut buf = Vec::new();
        img.write_to(&mut Cursor::new(&mut buf), image::ImageFormat::Jpeg)
            .unwrap();
        base64::engine::general_purpose::STANDARD.encode(&buf)
    }

    #[tokio::test]
    async fn test_process_file_full_pipeline() {
        use wiremock::matchers::{method, path};
        use wiremock::{Mock, MockServer, ResponseTemplate};

        let mock_server = MockServer::start().await;

        // Mock LLM title extraction.
        let title_body = serde_json::json!({
            "id": "chatcmpl-test",
            "object": "chat.completion",
            "model": "gpt-4o-mini",
            "choices": [{
                "index": 0,
                "message": {
                    "role": "assistant",
                    "content": "test-paper-doe-2023"
                },
                "finish_reason": "stop"
            }],
            "usage": {
                "prompt_tokens": 10,
                "completion_tokens": 5,
                "total_tokens": 15
            }
        });

        Mock::given(method("POST"))
            .and(path("/v1/chat/completions"))
            .respond_with(
                ResponseTemplate::new(200)
                    .insert_header("Content-Type", "application/json")
                    .set_body_json(&title_body),
            )
            .expect(1)
            .mount(&mock_server)
            .await;

        // Mock Mistral OCR with one page containing one image.
        let jpeg_b64 = make_test_jpeg_base64();
        let ocr_body = serde_json::json!({
            "model": "mistral-ocr-latest",
            "pages": [{
                "index": 0,
                "markdown": "# Test Content\n\n![Fig](img_0)",
                "images": [{
                    "id": "img_0",
                    "top_left_x": null,
                    "top_left_y": null,
                    "bottom_right_x": null,
                    "bottom_right_y": null,
                    "image_base64": jpeg_b64
                }]
            }],
            "usage_info": { "pages_processed": 1 }
        });

        Mock::given(method("POST"))
            .and(path("/v1/ocr"))
            .respond_with(
                ResponseTemplate::new(200)
                    .insert_header("Content-Type", "application/json")
                    .set_body_json(&ocr_body),
            )
            .expect(1)
            .mount(&mock_server)
            .await;

        // Set up temp dirs.
        let tmp = tempfile::TempDir::new().unwrap();
        let input_pdf = tmp.path().join("input.pdf");
        std::fs::write(&input_pdf, b"%PDF-fake-content").unwrap();
        let vault = tmp.path().join("vault");
        let papers = tmp.path().join("papers");

        let options = Options {
            lead: 0,
            trail: 0,
            dry_run: false,
        };
        let config = crate::config::Config {
            mistral_api_key: "sk-mistral-test".into(),
            openai_api_key: "sk-openai-test".into(),
            model: "gpt-4o-mini".into(),
            vault_path: vault.clone(),
            papers_path: papers.clone(),
            pdfium_path: PathBuf::from("/nonexistent/libpdfium.dylib"),
            openai_base_url: mock_server.uri(),
            mistral_base_url: mock_server.uri(),
        };
        let client = reqwest::Client::new();

        let handle = PdfiumHandle::Lazy(OnceLock::new());
        let result = process_file_inner(
            &input_pdf,
            &options,
            &config,
            &client,
            &MockPageText,
            &handle,
            &NoopProgress,
        )
        .await;

        assert!(
            result.is_ok(),
            "process_file_inner should succeed: {result:?}"
        );
        let pr = match result.unwrap() {
            ProcessOutcome::Written(pr) => pr,
            ProcessOutcome::DryRun { .. } => panic!("expected Written, got DryRun"),
        };

        // Title should be sanitized.
        assert_eq!(pr.title, "test-paper-doe-2023");

        // Markdown file should exist.
        assert_eq!(pr.markdown_path, vault.join("test-paper-doe-2023.md"));
        assert!(pr.markdown_path.exists(), "markdown file should exist");
        let md_contents = std::fs::read_to_string(&pr.markdown_path).unwrap();
        assert!(
            md_contents.contains("<!-- Page 0"),
            "markdown should contain page comment"
        );
        assert!(
            md_contents.contains("# Test Content"),
            "markdown should contain OCR content"
        );

        // PDF should be moved to papers.
        assert_eq!(pr.pdf_path, papers.join("test-paper-doe-2023.pdf"));
        assert!(pr.pdf_path.exists(), "PDF should be archived");
        assert!(!input_pdf.exists(), "original PDF should be moved");

        // Images directory should exist.
        assert!(pr.images_dir.is_some(), "images_dir should be Some");
        let img_dir = pr.images_dir.unwrap();
        assert!(img_dir.exists(), "images directory should exist");
        // Should contain at least one PNG file.
        let png_files: Vec<_> = std::fs::read_dir(&img_dir)
            .unwrap()
            .filter_map(|e| e.ok())
            .filter(|e| e.path().extension().is_some_and(|ext| ext == "png"))
            .collect();
        assert!(
            !png_files.is_empty(),
            "should have at least one PNG in images dir"
        );
    }

    #[tokio::test]
    async fn test_batch_continues_after_failure() {
        use wiremock::matchers::{method, path};
        use wiremock::{Mock, MockServer, ResponseTemplate};

        let mock_server = MockServer::start().await;

        // Mock LLM title extraction (called for each valid file).
        let title_body = serde_json::json!({
            "id": "chatcmpl-test",
            "object": "chat.completion",
            "model": "gpt-4o-mini",
            "choices": [{
                "index": 0,
                "message": {
                    "role": "assistant",
                    "content": "batch-test-paper-lee-etc-2022"
                },
                "finish_reason": "stop"
            }],
            "usage": {
                "prompt_tokens": 10,
                "completion_tokens": 5,
                "total_tokens": 15
            }
        });

        Mock::given(method("POST"))
            .and(path("/v1/chat/completions"))
            .respond_with(
                ResponseTemplate::new(200)
                    .insert_header("Content-Type", "application/json")
                    .set_body_json(&title_body),
            )
            .mount(&mock_server)
            .await;

        // Set up temp dirs.
        let tmp = tempfile::TempDir::new().unwrap();
        let valid1 = tmp.path().join("valid1.pdf");
        let missing = tmp.path().join("missing.pdf"); // Does not exist.
        let valid2 = tmp.path().join("valid2.pdf");
        std::fs::write(&valid1, b"%PDF-valid1").unwrap();
        // missing.pdf intentionally not created.
        std::fs::write(&valid2, b"%PDF-valid2").unwrap();

        let vault = tmp.path().join("vault");
        let papers = tmp.path().join("papers");

        let files = vec![valid1.clone(), missing.clone(), valid2.clone()];
        let options = Options {
            lead: 0,
            trail: 0,
            dry_run: true,
        };
        let config = crate::config::Config {
            mistral_api_key: "sk-mistral-test".into(),
            openai_api_key: "sk-openai-test".into(),
            model: "gpt-4o-mini".into(),
            vault_path: vault,
            papers_path: papers,
            pdfium_path: PathBuf::from("/nonexistent/libpdfium.dylib"),
            openai_base_url: mock_server.uri(),
            mistral_base_url: mock_server.uri(),
        };

        let results =
            process_batch_inner(&files, &options, &config, &MockPageText, &NoopProgress).await;

        assert_eq!(results.len(), 3, "should have results for all 3 files");

        // valid1.pdf should succeed.
        assert!(
            results[0].1.is_ok(),
            "valid1 should succeed: {:?}",
            results[0].1
        );

        // missing.pdf should fail with Io error.
        assert!(results[1].1.is_err(), "missing.pdf should fail");
        let err = results[1].1.as_ref().unwrap_err();
        assert!(
            matches!(err, crate::error::Error::Io(_)),
            "expected Io error for missing file, got: {err:?}"
        );

        // valid2.pdf should succeed (batch continued after failure).
        assert!(
            results[2].1.is_ok(),
            "valid2 should succeed: {:?}",
            results[2].1
        );
    }

    #[test]
    fn test_deduplicate_title_no_collision() {
        let dir = tempfile::TempDir::new().unwrap();
        let vault = dir.path().join("vault");
        let papers = dir.path().join("papers");
        std::fs::create_dir_all(&vault).unwrap();
        std::fs::create_dir_all(&papers).unwrap();

        let result = deduplicate_title("my-paper", &vault, &papers);
        assert_eq!(result, "my-paper");
    }

    #[test]
    fn test_deduplicate_title_md_collision() {
        let dir = tempfile::TempDir::new().unwrap();
        let vault = dir.path().join("vault");
        let papers = dir.path().join("papers");
        std::fs::create_dir_all(&vault).unwrap();
        std::fs::create_dir_all(&papers).unwrap();

        // Create a conflicting .md file.
        std::fs::write(vault.join("my-paper.md"), b"existing").unwrap();

        let result = deduplicate_title("my-paper", &vault, &papers);
        assert_eq!(result, "my-paper-2");
    }

    #[test]
    fn test_deduplicate_title_pdf_collision() {
        let dir = tempfile::TempDir::new().unwrap();
        let vault = dir.path().join("vault");
        let papers = dir.path().join("papers");
        std::fs::create_dir_all(&vault).unwrap();
        std::fs::create_dir_all(&papers).unwrap();

        // Create a conflicting .pdf file (but no .md).
        std::fs::write(papers.join("my-paper.pdf"), b"existing").unwrap();

        let result = deduplicate_title("my-paper", &vault, &papers);
        assert_eq!(result, "my-paper-2");
    }

    #[test]
    fn test_deduplicate_title_multiple_collisions() {
        let dir = tempfile::TempDir::new().unwrap();
        let vault = dir.path().join("vault");
        let papers = dir.path().join("papers");
        std::fs::create_dir_all(&vault).unwrap();
        std::fs::create_dir_all(&papers).unwrap();

        // untitled blocked by .md, untitled-2 blocked by .md, untitled-3 blocked by .pdf
        std::fs::write(vault.join("untitled.md"), b"a").unwrap();
        std::fs::write(vault.join("untitled-2.md"), b"b").unwrap();
        std::fs::write(papers.join("untitled-3.pdf"), b"c").unwrap();

        let result = deduplicate_title("untitled", &vault, &papers);
        assert_eq!(result, "untitled-4");
    }

    #[test]
    fn test_output_paths_with_nested_vault_and_papers() {
        let (md, pdf, img) = output_paths(
            "my-paper",
            Path::new("/home/user/Documents/Ekuro"),
            Path::new("/home/user/Documents/Papers"),
        );
        assert_eq!(md, PathBuf::from("/home/user/Documents/Ekuro/my-paper.md"));
        assert_eq!(
            pdf,
            PathBuf::from("/home/user/Documents/Papers/my-paper.pdf")
        );
        assert_eq!(
            img,
            PathBuf::from("/home/user/Documents/Ekuro/assets/images/my-paper")
        );
    }

    #[test]
    fn test_output_paths_untitled_fallback() {
        let (md, pdf, img) = output_paths("untitled", Path::new("/vault"), Path::new("/papers"));
        assert_eq!(md, PathBuf::from("/vault/untitled.md"));
        assert_eq!(pdf, PathBuf::from("/papers/untitled.pdf"));
        assert_eq!(img, PathBuf::from("/vault/assets/images/untitled"));
    }

    #[tokio::test]
    async fn test_batch_deduplicates_colliding_titles() {
        use wiremock::matchers::{method, path};
        use wiremock::{Mock, MockServer, ResponseTemplate};

        let mock_server = MockServer::start().await;

        // Mock LLM title extraction -- returns the same title for every call.
        let title_body = serde_json::json!({
            "id": "chatcmpl-test",
            "object": "chat.completion",
            "model": "gpt-4o-mini",
            "choices": [{
                "index": 0,
                "message": {
                    "role": "assistant",
                    "content": "batch-test-paper-lee-etc-2022"
                },
                "finish_reason": "stop"
            }],
            "usage": {
                "prompt_tokens": 10,
                "completion_tokens": 5,
                "total_tokens": 15
            }
        });

        Mock::given(method("POST"))
            .and(path("/v1/chat/completions"))
            .respond_with(
                ResponseTemplate::new(200)
                    .insert_header("Content-Type", "application/json")
                    .set_body_json(&title_body),
            )
            .mount(&mock_server)
            .await;

        // Mock Mistral OCR with a simple page (no images for simplicity).
        let ocr_body = serde_json::json!({
            "model": "mistral-ocr-latest",
            "pages": [{
                "index": 0,
                "markdown": "# Content",
                "images": []
            }],
            "usage_info": { "pages_processed": 1 }
        });

        Mock::given(method("POST"))
            .and(path("/v1/ocr"))
            .respond_with(
                ResponseTemplate::new(200)
                    .insert_header("Content-Type", "application/json")
                    .set_body_json(&ocr_body),
            )
            .mount(&mock_server)
            .await;

        // Set up temp dirs.
        let tmp = tempfile::TempDir::new().unwrap();
        let pdf1 = tmp.path().join("first.pdf");
        let pdf2 = tmp.path().join("second.pdf");
        std::fs::write(&pdf1, b"%PDF-first").unwrap();
        std::fs::write(&pdf2, b"%PDF-second").unwrap();
        let vault = tmp.path().join("vault");
        let papers = tmp.path().join("papers");

        let files = vec![pdf1.clone(), pdf2.clone()];
        let options = Options {
            lead: 0,
            trail: 0,
            dry_run: false,
        };
        let config = crate::config::Config {
            mistral_api_key: "sk-mistral-test".into(),
            openai_api_key: "sk-openai-test".into(),
            model: "gpt-4o-mini".into(),
            vault_path: vault.clone(),
            papers_path: papers.clone(),
            pdfium_path: PathBuf::from("/nonexistent/libpdfium.dylib"),
            openai_base_url: mock_server.uri(),
            mistral_base_url: mock_server.uri(),
        };

        let results =
            process_batch_inner(&files, &options, &config, &MockPageText, &NoopProgress).await;

        assert_eq!(results.len(), 2, "should have results for both files");

        // Both should succeed.
        let outcome1 = results[0].1.as_ref().expect("first file should succeed");
        let pr1 = match outcome1 {
            ProcessOutcome::Written(pr) => pr,
            ProcessOutcome::DryRun { .. } => panic!("expected Written, got DryRun"),
        };
        let outcome2 = results[1].1.as_ref().expect("second file should succeed");
        let pr2 = match outcome2 {
            ProcessOutcome::Written(pr) => pr,
            ProcessOutcome::DryRun { .. } => panic!("expected Written, got DryRun"),
        };

        // They should have different titles.
        assert_eq!(pr1.title, "batch-test-paper-lee-etc-2022");
        assert_eq!(pr2.title, "batch-test-paper-lee-etc-2022-2");

        // Both .md files should exist.
        assert!(
            pr1.markdown_path.exists(),
            "first markdown should exist: {}",
            pr1.markdown_path.display()
        );
        assert!(
            pr2.markdown_path.exists(),
            "second markdown should exist: {}",
            pr2.markdown_path.display()
        );

        // Both .pdf files should exist in papers/.
        assert!(
            pr1.pdf_path.exists(),
            "first pdf should exist: {}",
            pr1.pdf_path.display()
        );
        assert!(
            pr2.pdf_path.exists(),
            "second pdf should exist: {}",
            pr2.pdf_path.display()
        );

        // Paths should differ.
        assert_ne!(pr1.markdown_path, pr2.markdown_path);
        assert_ne!(pr1.pdf_path, pr2.pdf_path);
    }

    #[test]
    fn test_output_paths_constructs_correct_paths() {
        let (md, pdf, img) = output_paths(
            "attention-is-all-you-need",
            Path::new("/vault"),
            Path::new("/papers"),
        );
        assert_eq!(md, PathBuf::from("/vault/attention-is-all-you-need.md"));
        assert_eq!(pdf, PathBuf::from("/papers/attention-is-all-you-need.pdf"));
        assert_eq!(
            img,
            PathBuf::from("/vault/assets/images/attention-is-all-you-need")
        );
    }

    #[test]
    fn test_deduplicate_title_images_dir_collision() {
        let dir = tempfile::TempDir::new().unwrap();
        let vault = dir.path().join("vault");
        let papers = dir.path().join("papers");
        std::fs::create_dir_all(&vault).unwrap();
        std::fs::create_dir_all(&papers).unwrap();

        // Pre-create ONLY the images directory (no .md, no .pdf).
        let images_dir = vault.join("assets").join("images").join("my-paper");
        std::fs::create_dir_all(&images_dir).unwrap();
        // Place a sentinel file to verify the stale dir is untouched later.
        let sentinel = images_dir.join("stale-image.png");
        std::fs::write(&sentinel, b"stale").unwrap();

        let result = deduplicate_title("my-paper", &vault, &papers);
        assert_eq!(result, "my-paper-2", "should suffix when images dir exists");

        // Stale directory must be untouched.
        assert!(sentinel.exists(), "stale sentinel file must still exist");
        assert_eq!(std::fs::read(&sentinel).unwrap(), b"stale");
    }

    #[test]
    fn test_deduplicate_title_images_dir_chain_collision() {
        let dir = tempfile::TempDir::new().unwrap();
        let vault = dir.path().join("vault");
        let papers = dir.path().join("papers");
        std::fs::create_dir_all(&vault).unwrap();
        std::fs::create_dir_all(&papers).unwrap();

        // "paper" blocked by .md, "paper-2" blocked by images dir, "paper-3" blocked by .pdf
        std::fs::write(vault.join("paper.md"), b"a").unwrap();
        let images_2 = vault.join("assets").join("images").join("paper-2");
        std::fs::create_dir_all(&images_2).unwrap();
        std::fs::write(papers.join("paper-3.pdf"), b"c").unwrap();

        let result = deduplicate_title("paper", &vault, &papers);
        assert_eq!(result, "paper-4");
    }

    #[test]
    fn test_options_default() {
        let opts = Options::default();
        assert_eq!(opts.lead, 0);
        assert_eq!(opts.trail, 0);
        assert!(!opts.dry_run);
    }

    #[test]
    fn test_process_outcome_dry_run_is_debug() {
        let outcome = ProcessOutcome::DryRun {
            title: "foo".into(),
            md_path: PathBuf::from("/vault/foo.md"),
            pdf_path: PathBuf::from("/papers/foo.pdf"),
        };
        let dbg = format!("{outcome:?}");
        assert!(dbg.contains("DryRun"));
        assert!(dbg.contains("foo"));
    }

    #[test]
    fn test_process_outcome_written_is_debug() {
        let outcome = ProcessOutcome::Written(ProcessResult {
            title: "bar".into(),
            markdown_path: PathBuf::from("/vault/bar.md"),
            pdf_path: PathBuf::from("/papers/bar.pdf"),
            images_dir: None,
        });
        let dbg = format!("{outcome:?}");
        assert!(dbg.contains("Written"));
        assert!(dbg.contains("bar"));
    }

    #[test]
    fn test_options_from_cli() {
        let cli = crate::cli::Cli {
            files: vec![PathBuf::from("test.pdf")],
            lead: 2,
            trail: 3,
            vault: Some(PathBuf::from("/vault")),
            papers: Some(PathBuf::from("/papers")),
            model: Some("gpt-4o".into()),
            dry_run: true,
        };
        let opts = Options::from(&cli);
        assert_eq!(opts.lead, 2);
        assert_eq!(opts.trail, 3);
        assert!(opts.dry_run);
    }
}
