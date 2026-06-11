// Mistral OCR post-processing: per-page image saving, markdown image ref replacement,
// page comment headers, and double-newline page joining.

use std::collections::HashMap;
use std::path::Path;
use std::sync::LazyLock;

use base64::Engine as _;
use regex::Regex;

use crate::error::Result;
use crate::ocr::OcrPage;

/// Regex matching markdown image syntax: `![alt](src)`.
static RE_MD_IMAGE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"!\[([^\]]*)\]\(([^)]+)\)").unwrap());

/// Result of post-processing a full OCR response.
pub struct PostprocessedOutput {
    /// The combined markdown text (all pages joined with double newlines).
    pub markdown: String,
    /// List of image file paths that were saved (relative to `output_dir`).
    pub saved_images: Vec<String>,
}

/// Post-process OCR pages: save images as PNG files, replace image references
/// in markdown, prepend page comment headers, and join pages.
pub fn postprocess(
    pages: &[OcrPage],
    output_dir: &Path,
    stem: &str,
) -> Result<PostprocessedOutput> {
    std::fs::create_dir_all(output_dir)?;

    let mut page_blocks: Vec<String> = Vec::new();
    let mut all_saved: Vec<String> = Vec::new();

    for page in pages {
        let mut id_to_path: HashMap<String, String> = HashMap::new();

        // Save each image that has base64 data
        for img in &page.images {
            if let Some(ref b64) = img.image_base64 {
                let sanitized_id = sanitize_image_id(&img.id);
                let filename = format!("page_{}_{}.png", page.index, sanitized_id);
                let abs_path = output_dir.join(&filename);
                save_image_as_png(b64, &abs_path)?;
                let rel_path = format!("{stem}/{filename}");
                id_to_path.insert(img.id.clone(), rel_path.clone());
                all_saved.push(rel_path);
            }
        }

        // Replace image refs in markdown
        let processed_md = replace_image_refs(&page.markdown, &id_to_path);

        // Build page block
        let comment = page_comment(page.index, page.images.len());
        page_blocks.push(format!("{comment}\n{processed_md}"));
    }

    Ok(PostprocessedOutput {
        markdown: page_blocks.join("\n\n"),
        saved_images: all_saved,
    })
}

/// Build the `<!-- Page N - M images -->` comment header.
fn page_comment(index: u32, image_count: usize) -> String {
    format!("<!-- Page {index} - {image_count} images -->")
}

/// Strip `data:...;base64,` URI prefix from a base64 string, if present.
/// Returns the raw base64 payload.
fn strip_data_uri_prefix(s: &str) -> &str {
    if s.starts_with("data:") {
        match s.find(',') {
            Some(pos) => &s[pos + 1..],
            None => s,
        }
    } else {
        s
    }
}

/// Sanitize an image ID by removing common image file extensions.
fn sanitize_image_id(id: &str) -> String {
    id.replace(".jpeg", "")
        .replace(".jpg", "")
        .replace(".png", "")
}

/// Save a single base64-encoded image as PNG to `output_path`.
fn save_image_as_png(base64_data: &str, output_path: &Path) -> Result<()> {
    let raw_b64 = strip_data_uri_prefix(base64_data);
    let bytes = base64::engine::general_purpose::STANDARD.decode(raw_b64)?;
    let img = image::load_from_memory(&bytes)?;
    img.save_with_format(output_path, image::ImageFormat::Png)?;
    Ok(())
}

/// Replace `![alt](src)` references in markdown text where `src` matches
/// a known image ID, substituting the saved relative path.
fn replace_image_refs(markdown: &str, id_to_path: &HashMap<String, String>) -> String {
    RE_MD_IMAGE
        .replace_all(markdown, |caps: &regex::Captures| {
            let alt = &caps[1];
            let src = &caps[2];
            match id_to_path.get(src) {
                Some(path) => format!("![{alt}]({path})"),
                None => caps[0].to_string(),
            }
        })
        .into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ocr::OcrPage;

    #[test]
    fn test_postprocess_single_page_no_images() {
        let tmp_dir = tempfile::TempDir::new().unwrap();
        let pages = vec![OcrPage {
            index: 0,
            markdown: "# Hello".into(),
            images: vec![],
            dimensions: None,
        }];

        let result = postprocess(&pages, tmp_dir.path(), "test-stem").unwrap();

        assert!(
            result.markdown.starts_with("<!-- Page 0 - 0 images -->"),
            "markdown should start with page comment, got: {:?}",
            result.markdown
        );
        assert!(
            result.markdown.contains("# Hello"),
            "markdown should contain original content"
        );
        assert!(result.saved_images.is_empty(), "no images should be saved");

        // No files should be created in the temp dir (apart from the dir itself)
        let entries: Vec<_> = std::fs::read_dir(tmp_dir.path()).unwrap().collect();
        assert!(
            entries.is_empty(),
            "no files should be created in output dir"
        );
    }

    /// Generate a minimal 1x1 red JPEG as base64 for testing.
    fn make_test_jpeg_base64() -> String {
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

    #[test]
    fn test_postprocess_single_page_with_image() {
        use crate::ocr::OcrImage;

        let tmp_dir = tempfile::TempDir::new().unwrap();
        let jpeg_b64 = make_test_jpeg_base64();

        let pages = vec![OcrPage {
            index: 0,
            markdown: "![Fig](img_0)".into(),
            images: vec![OcrImage {
                id: "img_0".into(),
                top_left_x: None,
                top_left_y: None,
                bottom_right_x: None,
                bottom_right_y: None,
                image_base64: Some(jpeg_b64),
            }],
            dimensions: None,
        }];

        let result = postprocess(&pages, tmp_dir.path(), "test-stem").unwrap();

        // Check file was saved
        let saved_path = tmp_dir.path().join("page_0_img_0.png");
        assert!(saved_path.exists(), "PNG file should be saved on disk");

        // Verify it is a valid PNG (magic bytes: 0x89 P N G)
        let file_bytes = std::fs::read(&saved_path).unwrap();
        assert!(
            file_bytes.starts_with(&[0x89, b'P', b'N', b'G']),
            "saved file should be valid PNG"
        );

        // Check markdown reference was replaced
        assert!(
            result
                .markdown
                .contains("![Fig](test-stem/page_0_img_0.png)"),
            "image ref should be replaced, got: {:?}",
            result.markdown
        );

        // Check saved_images list
        assert_eq!(result.saved_images, vec!["test-stem/page_0_img_0.png"]);
    }

    #[test]
    fn test_image_ref_regex_replacement() {
        let mut map = HashMap::new();
        map.insert(
            "img_0.jpeg".to_string(),
            "stem/page_0_img_0.png".to_string(),
        );
        map.insert("img_1".to_string(), "stem/page_0_img_1.png".to_string());

        // Single replacement
        let result = replace_image_refs("![Fig 1](img_0.jpeg)", &map);
        assert_eq!(result, "![Fig 1](stem/page_0_img_0.png)");

        // Multiple replacements in one string
        let input = "Text ![Fig 1](img_0.jpeg) middle ![Fig 2](img_1) end";
        let result = replace_image_refs(input, &map);
        assert_eq!(
            result,
            "Text ![Fig 1](stem/page_0_img_0.png) middle ![Fig 2](stem/page_0_img_1.png) end"
        );

        // Alt text with special characters preserved
        let result = replace_image_refs("![Fig (a) & b](img_0.jpeg)", &map);
        assert_eq!(result, "![Fig (a) & b](stem/page_0_img_0.png)");
    }

    #[test]
    fn test_unknown_image_id_left_unchanged() {
        let mut map = HashMap::new();
        map.insert("img_0".to_string(), "stem/page_0_img_0.png".to_string());

        // Unknown ID should be left as-is
        let result = replace_image_refs("![Fig](unknown_img)", &map);
        assert_eq!(result, "![Fig](unknown_img)");

        // Mixed: known and unknown
        let input = "![A](img_0) text ![B](unknown_img)";
        let result = replace_image_refs(input, &map);
        assert_eq!(result, "![A](stem/page_0_img_0.png) text ![B](unknown_img)");
    }

    #[test]
    fn test_strip_data_uri_prefix() {
        // JPEG data URI
        assert_eq!(
            strip_data_uri_prefix("data:image/jpeg;base64,/9j/4AAQ"),
            "/9j/4AAQ"
        );
        // PNG data URI
        assert_eq!(
            strip_data_uri_prefix("data:image/png;base64,iVBOR"),
            "iVBOR"
        );
        // No prefix -- returned unchanged
        assert_eq!(strip_data_uri_prefix("iVBOR"), "iVBOR");
        // Arbitrary MIME type
        assert_eq!(
            strip_data_uri_prefix("data:application/octet-stream;base64,AQID"),
            "AQID"
        );
    }

    #[test]
    fn test_multipage_double_newline_join() {
        let tmp_dir = tempfile::TempDir::new().unwrap();
        let pages = vec![
            OcrPage {
                index: 0,
                markdown: "Page zero content".into(),
                images: vec![],
                dimensions: None,
            },
            OcrPage {
                index: 1,
                markdown: "Page one content".into(),
                images: vec![],
                dimensions: None,
            },
            OcrPage {
                index: 2,
                markdown: "Page two content".into(),
                images: vec![],
                dimensions: None,
            },
        ];

        let result = postprocess(&pages, tmp_dir.path(), "stem").unwrap();

        // Should contain all three page comments
        assert!(result.markdown.contains("<!-- Page 0 - 0 images -->"));
        assert!(result.markdown.contains("<!-- Page 1 - 0 images -->"));
        assert!(result.markdown.contains("<!-- Page 2 - 0 images -->"));

        // Verify exact structure: comment\nmarkdown\n\ncomment\nmarkdown\n\ncomment\nmarkdown
        let expected = "\
<!-- Page 0 - 0 images -->\n\
Page zero content\n\
\n\
<!-- Page 1 - 0 images -->\n\
Page one content\n\
\n\
<!-- Page 2 - 0 images -->\n\
Page two content";

        assert_eq!(result.markdown, expected);
    }

    #[test]
    fn test_sanitize_image_id() {
        assert_eq!(sanitize_image_id("img_0.jpeg"), "img_0");
        assert_eq!(sanitize_image_id("img_0.jpg"), "img_0");
        assert_eq!(sanitize_image_id("img_0.png"), "img_0");
        assert_eq!(sanitize_image_id("img_0"), "img_0");
        assert_eq!(sanitize_image_id("figure.1.jpeg"), "figure.1");
    }
}
