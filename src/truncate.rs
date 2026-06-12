use std::path::Path;

use crate::error::{Error, Result};
use crate::pipeline::PdfiumHandle;

/// Truncate leading and trailing pages from a PDF file.
///
/// If `lead == 0 && trail == 0`, the file bytes are returned directly
/// without touching Pdfium (fast path).
///
/// Returns the modified PDF bytes as a `Vec<u8>`.
pub(crate) fn truncate_pdf(
    path: &Path,
    lead: usize,
    trail: usize,
    pdfium: &PdfiumHandle<'_>,
    pdfium_path: &Path,
) -> Result<Vec<u8>> {
    if lead == 0 && trail == 0 {
        let bytes = std::fs::read(path)?;
        return Ok(bytes);
    }

    let input = std::fs::read(path)?;
    let pdfium = pdfium.get_or_init(pdfium_path)?;
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
    use std::sync::OnceLock;

    #[test]
    fn test_truncate_pdf_zero_zero_returns_original_bytes() {
        let content = b"%PDF-fake-content-for-test";
        let tmp = tempfile::NamedTempFile::new().unwrap();
        std::io::Write::write_all(&mut &tmp, content).unwrap();

        let pdfium = OnceLock::new();
        let handle = PdfiumHandle::Lazy(&pdfium);
        let dummy_pdfium = std::path::Path::new("/nonexistent/libpdfium.dylib");
        let result = truncate_pdf(tmp.path(), 0, 0, &handle, dummy_pdfium).unwrap();
        assert_eq!(result, content);
    }

    #[test]
    #[ignore]
    fn test_truncate_pdf_lead_trail_exceeds_pages_returns_error() {
        let path = std::path::Path::new("tests/fixtures/sample-5page.pdf");
        let pp = pdfium_path();
        let pdfium_inst = lmpdf::Pdfium::open(&pp).unwrap();
        let page_count = {
            let doc = pdfium_inst.open_document(path, None).unwrap();
            doc.page_count()
        };
        let handle = PdfiumHandle::Borrowed(&pdfium_inst);
        let result = truncate_pdf(path, page_count, 1, &handle, &pp);
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
        let pp = pdfium_path();
        let pdfium_inst = lmpdf::Pdfium::open(&pp).unwrap();
        let original = {
            let doc = pdfium_inst.open_document(path, None).unwrap();
            let count = doc.page_count();
            assert!(
                count >= 3,
                "need >= 3 pages for truncate(1,1) test, got {count}"
            );
            count
        };
        let handle = PdfiumHandle::Borrowed(&pdfium_inst);
        let result = truncate_pdf(path, 1, 1, &handle, &pp).unwrap();
        assert!(result.starts_with(b"%PDF"), "output should be valid PDF");
        let doc2 = pdfium_inst.load_document(&result, None).unwrap();
        assert_eq!(doc2.page_count(), original - 2);
    }

    /// Resolve the Pdfium library path from `PDFIUM_PATH` env var.
    fn pdfium_path() -> std::path::PathBuf {
        std::path::PathBuf::from(
            std::env::var("PDFIUM_PATH").expect("Set PDFIUM_PATH to run Pdfium tests"),
        )
    }
}
