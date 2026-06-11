// Pipeline orchestration: process PDF files through 5 steps.

use std::path::{Path, PathBuf};

use crate::cli::Cli;
use crate::config::Config;
use crate::error::Result;

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

const OPENAI_BASE_URL: &str = "https://api.openai.com";
const MISTRAL_BASE_URL: &str = "https://api.mistral.ai";

/// Default page_text implementation using pdfium.
fn default_page_text(pdf_bytes: &[u8], pdfium_path: &Path) -> Result<String> {
    let pdfium = lmpdf::Pdfium::open(pdfium_path)?;
    let doc = pdfium.load_document(pdf_bytes, None)?;
    doc.page_text(0).map_err(Into::into)
}

/// Public entry point: process a single PDF file through the 5-step pipeline.
pub async fn process_file(
    input: &Path,
    cli: &Cli,
    config: &Config,
    client: &reqwest::Client,
) -> Result<Option<ProcessResult>> {
    process_file_with(
        input,
        cli,
        config,
        client,
        default_page_text,
        OPENAI_BASE_URL,
        MISTRAL_BASE_URL,
    )
    .await
}

/// Testable inner implementation with injectable page_text function and base URLs.
async fn process_file_with(
    input: &Path,
    cli: &Cli,
    config: &Config,
    client: &reqwest::Client,
    page_text_fn: fn(&[u8], &Path) -> Result<String>,
    openai_base_url: &str,
    mistral_base_url: &str,
) -> Result<Option<ProcessResult>> {
    let filename = input.file_name().unwrap_or_default().to_string_lossy();

    // Step 1: Truncate
    eprintln!("[1/5] Truncating {filename}...");
    let pdf_bytes = crate::truncate::truncate_pdf(input, cli.lead, cli.trail, &config.pdfium_path)?;

    // Step 2: Extract title
    eprintln!("[2/5] Extracting title...");
    let page_text = page_text_fn(&pdf_bytes, &config.pdfium_path)?;
    let title = crate::title::extract_title(
        &page_text,
        &config.model,
        &config.openai_api_key,
        openai_base_url,
    )
    .await?;

    if cli.dry_run {
        let (md, pdf, _img) = output_paths(&title, &config.vault_path, &config.papers_path);
        eprintln!("[dry-run] Proposed filename: {title}");
        eprintln!("[dry-run]   markdown: {}", md.display());
        eprintln!("[dry-run]   pdf:      {}", pdf.display());
        return Ok(None);
    }

    // Step 3: Mistral OCR
    eprintln!("[3/5] Running OCR...");
    let ocr_response = crate::ocr::ocr_pdf(
        client,
        mistral_base_url,
        &config.mistral_api_key,
        &pdf_bytes,
        true,
    )
    .await?;

    // Step 4: Post-process
    eprintln!("[4/5] Post-processing...");
    let (md_path, pdf_path, images_dir) =
        output_paths(&title, &config.vault_path, &config.papers_path);
    let output = crate::postproc::postprocess(&ocr_response.pages, &images_dir, &title)?;

    // Step 5: Move outputs
    eprintln!("[5/5] Writing outputs...");
    if let Some(parent) = md_path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(&md_path, &output.markdown)?;

    if let Some(parent) = pdf_path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    move_file(input, &pdf_path)?;

    let has_images = !output.saved_images.is_empty();

    Ok(Some(ProcessResult {
        title,
        markdown_path: md_path,
        pdf_path,
        images_dir: if has_images { Some(images_dir) } else { None },
    }))
}

/// Process all files in batch mode. Continues past individual failures.
/// Returns a vec of (path, result) pairs.
pub async fn process_batch(
    cli: &Cli,
    config: &Config,
) -> Vec<(PathBuf, Result<Option<ProcessResult>>)> {
    process_batch_with(
        cli,
        config,
        default_page_text,
        OPENAI_BASE_URL,
        MISTRAL_BASE_URL,
    )
    .await
}

/// Testable batch processing with injectable dependencies.
async fn process_batch_with(
    cli: &Cli,
    config: &Config,
    page_text_fn: fn(&[u8], &Path) -> Result<String>,
    openai_base_url: &str,
    mistral_base_url: &str,
) -> Vec<(PathBuf, Result<Option<ProcessResult>>)> {
    let client = reqwest::Client::new();
    let mut results = Vec::new();
    for path in &cli.files {
        eprintln!("\n=== Processing: {} ===", path.display());
        let result = process_file_with(
            path,
            cli,
            config,
            &client,
            page_text_fn,
            openai_base_url,
            mistral_base_url,
        )
        .await;
        if let Err(ref e) = result {
            eprintln!("ERROR processing {}: {e}", path.display());
        }
        results.push((path.clone(), result));
    }
    results
}

/// Move a file from `src` to `dst`, trying `fs::rename` first and falling back
/// to `fs::copy` + `fs::remove_file` for cross-device moves.
fn move_file(src: &Path, dst: &Path) -> Result<()> {
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

    fn mock_page_text(_pdf_bytes: &[u8], _pdfium_path: &Path) -> crate::error::Result<String> {
        Ok("fake page text for title extraction".into())
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
                    "content": "Test Title From LLM"
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

        let cli = crate::cli::Cli {
            files: vec![input_pdf.clone()],
            lead: 0,
            trail: 0,
            vault: vault.clone(),
            papers: papers.clone(),
            model: "gpt-4o-mini".into(),
            dry_run: true,
            verbose: false,
        };
        let config = crate::config::Config {
            mistral_api_key: "sk-mistral-test".into(),
            openai_api_key: "sk-openai-test".into(),
            model: "gpt-4o-mini".into(),
            vault_path: vault,
            papers_path: papers,
            pdfium_path: PathBuf::from("/nonexistent/libpdfium.dylib"),
        };
        let client = reqwest::Client::new();

        let result = process_file_with(
            &input_pdf,
            &cli,
            &config,
            &client,
            mock_page_text,
            &mock_server.uri(),
            &mock_server.uri(),
        )
        .await;

        assert!(
            result.is_ok(),
            "process_file_with should succeed: {result:?}"
        );
        assert!(result.unwrap().is_none(), "dry-run should return Ok(None)");
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
                    "content": "Test Paper"
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

        let cli = crate::cli::Cli {
            files: vec![input_pdf.clone()],
            lead: 0,
            trail: 0,
            vault: vault.clone(),
            papers: papers.clone(),
            model: "gpt-4o-mini".into(),
            dry_run: false,
            verbose: false,
        };
        let config = crate::config::Config {
            mistral_api_key: "sk-mistral-test".into(),
            openai_api_key: "sk-openai-test".into(),
            model: "gpt-4o-mini".into(),
            vault_path: vault.clone(),
            papers_path: papers.clone(),
            pdfium_path: PathBuf::from("/nonexistent/libpdfium.dylib"),
        };
        let client = reqwest::Client::new();

        let result = process_file_with(
            &input_pdf,
            &cli,
            &config,
            &client,
            mock_page_text,
            &mock_server.uri(),
            &mock_server.uri(),
        )
        .await;

        assert!(
            result.is_ok(),
            "process_file_with should succeed: {result:?}"
        );
        let pr = result.unwrap().expect("should return Some(ProcessResult)");

        // Title should be sanitized.
        assert_eq!(pr.title, "test-paper");

        // Markdown file should exist.
        assert_eq!(pr.markdown_path, vault.join("test-paper.md"));
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
        assert_eq!(pr.pdf_path, papers.join("test-paper.pdf"));
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
                    "content": "Batch Test Paper"
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

        let cli = crate::cli::Cli {
            files: vec![valid1.clone(), missing.clone(), valid2.clone()],
            lead: 0,
            trail: 0,
            vault: vault.clone(),
            papers: papers.clone(),
            model: "gpt-4o-mini".into(),
            dry_run: true, // Use dry-run so we don't need OCR mock.
            verbose: false,
        };
        let config = crate::config::Config {
            mistral_api_key: "sk-mistral-test".into(),
            openai_api_key: "sk-openai-test".into(),
            model: "gpt-4o-mini".into(),
            vault_path: vault,
            papers_path: papers,
            pdfium_path: PathBuf::from("/nonexistent/libpdfium.dylib"),
        };

        let results = process_batch_with(
            &cli,
            &config,
            mock_page_text,
            &mock_server.uri(),
            &mock_server.uri(),
        )
        .await;

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
}
