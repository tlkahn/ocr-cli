// PDF truncation wrapper around lmpdf.

use std::path::Path;

use crate::error::{Error, Result};

/// Truncate leading and trailing pages from a PDF file.
///
/// If `lead == 0 && trail == 0`, the file bytes are returned directly
/// without loading Pdfium (fast path).
///
/// Returns the modified PDF bytes as a `Vec<u8>`.
pub fn truncate_pdf(path: &Path, lead: usize, trail: usize, pdfium_path: &Path) -> Result<Vec<u8>> {
    if lead == 0 && trail == 0 {
        let bytes = std::fs::read(path)?;
        return Ok(bytes);
    }

    let input = std::fs::read(path)?;
    let pdfium = lmpdf::Pdfium::open(pdfium_path)?;
    let mut doc = pdfium.load_document(&input, None)?;

    doc.truncate(lead, trail).map_err(|e| match &e {
        lmpdf::Error::Document(lmpdf::error::DocumentError::TruncationError(msg)) => {
            Error::Truncation(msg.clone())
        }
        _ => Error::Pdf(e),
    })?;

    let bytes = doc.save_to_vec()?;
    Ok(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Resolve the Pdfium library path from `PDFIUM_PATH` env var.
    fn pdfium_path() -> std::path::PathBuf {
        std::path::PathBuf::from(
            std::env::var("PDFIUM_PATH").expect("Set PDFIUM_PATH to run Pdfium tests"),
        )
    }

    #[test]
    fn test_truncate_pdf_zero_zero_returns_original_bytes() {
        let content = b"%PDF-fake-content-for-test";
        let tmp = tempfile::NamedTempFile::new().unwrap();
        std::io::Write::write_all(&mut &tmp, content).unwrap();

        // pdfium_path is unused in the lead=0/trail=0 fast path, so any path works.
        let dummy_pdfium = std::path::Path::new("/nonexistent/libpdfium.dylib");
        let result = truncate_pdf(tmp.path(), 0, 0, dummy_pdfium).unwrap();
        assert_eq!(result, content);
    }

    #[test]
    #[ignore]
    fn test_truncate_pdf_lead_trail_exceeds_pages_returns_error() {
        let path = std::path::Path::new("tests/fixtures/sample-5page.pdf");
        let pdfium = pdfium_path();
        // Determine page count in its own scope to fully drop the Pdfium
        // instance before truncate_pdf opens a new one.
        let page_count = {
            let pdfium_lib = lmpdf::Pdfium::open(&pdfium).unwrap();
            let doc = pdfium_lib.open_document(path, None).unwrap();
            doc.page_count()
        };
        // lead + trail >= page_count should error
        let result = truncate_pdf(path, page_count, 1, &pdfium);
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(
            matches!(err, crate::error::Error::Truncation(_)),
            "expected Error::Truncation, got: {err:?}"
        );
    }

    #[test]
    #[ignore]
    fn test_truncate_pdf_integration_reduces_page_count() {
        let path = std::path::Path::new("tests/fixtures/sample-5page.pdf");
        let pdfium = pdfium_path();
        // Read original page count in its own scope so the Pdfium instance
        // is fully dropped before truncate_pdf opens a new one.
        let original = {
            let pdfium_lib = lmpdf::Pdfium::open(&pdfium).unwrap();
            let doc = pdfium_lib.open_document(path, None).unwrap();
            let count = doc.page_count();
            assert!(
                count >= 3,
                "need >= 3 pages for truncate(1,1) test, got {count}"
            );
            count
        };
        // Truncate 1 page from each end
        let result = truncate_pdf(path, 1, 1, &pdfium).unwrap();
        // Verify output starts with PDF magic bytes
        assert!(result.starts_with(b"%PDF"), "output should be valid PDF");
        // Reload and check page count
        let pdfium_lib = lmpdf::Pdfium::open(&pdfium).unwrap();
        let doc2 = pdfium_lib.load_document(&result, None).unwrap();
        assert_eq!(doc2.page_count(), original - 2);
    }
}
