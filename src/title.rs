// Title extraction and filename sanitization.

use std::sync::LazyLock;

use futures::StreamExt;
use llm_core::{Prompt, Provider, collect_text, stream::Chunk};
use llm_openai::provider::OpenAiProvider;
use regex::Regex;

const TITLE_SYSTEM_PROMPT: &str = "\
You are a bibliographic metadata extractor. Given the text of the first page of an academic paper or book, \
extract the document title, author name(s), and publication year. \
Return ONLY a filename stem in the format: book-title-first-author-year \
(all lowercase, words separated by hyphens, no file extension). \
If there are multiple authors, use the first author's last name followed by 'etc'. \
If the year is not present on the page, omit it. \
Do not include quotes, formatting, or any other text — just the single filename stem on one line.";

static RE_ZLIB: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)\(z-library\)").unwrap());

/// Sanitize a raw title string into a safe, lowercase, hyphenated filename stem.
pub fn sanitize_filename(raw: &str) -> String {
    // Strip Z-Library noise (case-insensitive).
    let s = RE_ZLIB.replace_all(raw, "");

    // Remove non-alphanumeric, non-whitespace, non-hyphen chars.
    let s: String = s
        .chars()
        .filter(|c| c.is_alphanumeric() || c.is_whitespace() || *c == '-')
        .collect();

    // Lowercase.
    let s = s.to_lowercase();

    // Collapse whitespace to hyphens.
    let parts: Vec<&str> = s.split_whitespace().collect();
    let s = parts.join("-");

    // Trim leading/trailing hyphens.
    let s = s.trim_matches('-').to_string();

    // Truncate to at most 60 characters on a hyphen boundary.
    let s = if s.chars().count() > 60 {
        let truncated: String = s.chars().take(60).collect();
        match truncated.rfind('-') {
            Some(pos) => truncated[..pos].to_string(),
            None => truncated,
        }
    } else {
        s
    };

    // Trim trailing hyphens again after truncation.
    let s = s.trim_end_matches('-').to_string();

    // Empty check.
    if s.is_empty() {
        "untitled".to_string()
    } else {
        s
    }
}

/// Extract a short academic title from the first page's text using an LLM.
///
/// Sends the page text to the OpenAI-compatible provider with a system prompt
/// instructing it to return only the paper title, nothing else.
/// The raw LLM response is then passed through `sanitize_filename()`.
pub async fn extract_title(
    page_text: &str,
    model: &str,
    api_key: &str,
    base_url: &str,
) -> crate::error::Result<String> {
    let provider = OpenAiProvider::new(base_url);
    let prompt = Prompt::new(page_text)
        .with_system(TITLE_SYSTEM_PROMPT)
        .with_option("temperature", serde_json::json!(0));

    let stream = provider
        .execute(model, &prompt, Some(api_key), false)
        .await?;
    let chunks: Vec<Chunk> = stream
        .collect::<Vec<_>>()
        .await
        .into_iter()
        .filter_map(|r| r.ok())
        .collect();

    let raw_title = collect_text(&chunks);
    let title = sanitize_filename(&raw_title);
    Ok(title)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sanitize_strips_quotes() {
        assert_eq!(
            sanitize_filename("\"Attention Is All You Need\""),
            "attention-is-all-you-need"
        );
    }

    #[test]
    fn test_sanitize_strips_zlibrary_noise() {
        assert_eq!(sanitize_filename("Some Title (Z-Library)"), "some-title");
        assert_eq!(
            sanitize_filename("Another (z-library) Book"),
            "another-book"
        );
    }

    #[test]
    fn test_sanitize_empty_input() {
        assert_eq!(sanitize_filename(""), "untitled");
        assert_eq!(sanitize_filename("   "), "untitled");
        assert_eq!(sanitize_filename("\"\"\""), "untitled");
    }

    #[test]
    fn test_sanitize_normalizes_unicode_punctuation() {
        // Curly double quotes (U+201C, U+201D).
        assert_eq!(
            sanitize_filename("\u{201C}Hello World\u{201D}"),
            "hello-world"
        );
        // Right single quote / apostrophe (U+2019).
        assert_eq!(sanitize_filename("It\u{2019}s a Test"), "its-a-test");
    }

    #[test]
    fn test_sanitize_truncates_to_60_chars() {
        let long = "A Very Long Title That Goes On And On And On And On Until It Exceeds Sixty Characters Easily";
        let result = sanitize_filename(long);
        assert!(
            result.chars().count() <= 60,
            "result was {} chars: {result}",
            result.chars().count()
        );
        // Must not end mid-word (no trailing hyphen).
        assert!(!result.ends_with('-'), "result should not end with hyphen");
        // Must still be a valid non-empty string.
        assert!(!result.is_empty());
    }

    #[tokio::test]
    async fn test_extract_title_returns_sanitized_title() {
        use wiremock::matchers::{method, path};
        use wiremock::{Mock, MockServer, ResponseTemplate};

        let mock_server = MockServer::start().await;

        let body = serde_json::json!({
            "id": "chatcmpl-test",
            "object": "chat.completion",
            "model": "gpt-4o-mini",
            "choices": [{
                "index": 0,
                "message": {
                    "role": "assistant",
                    "content": "attention-is-all-you-need-vaswani-etc-2017"
                },
                "finish_reason": "stop"
            }],
            "usage": {
                "prompt_tokens": 100,
                "completion_tokens": 10,
                "total_tokens": 110
            }
        });

        Mock::given(method("POST"))
            .and(path("/v1/chat/completions"))
            .respond_with(
                ResponseTemplate::new(200)
                    .insert_header("Content-Type", "application/json")
                    .set_body_json(&body),
            )
            .expect(1)
            .mount(&mock_server)
            .await;

        let result = extract_title(
            "some page text...",
            "gpt-4o-mini",
            "sk-test",
            &mock_server.uri(),
        )
        .await;

        assert!(result.is_ok(), "extract_title failed: {result:?}");
        assert_eq!(
            result.unwrap(),
            "attention-is-all-you-need-vaswani-etc-2017"
        );
    }

    #[tokio::test]
    async fn test_extract_title_propagates_llm_error() {
        use wiremock::matchers::{method, path};
        use wiremock::{Mock, MockServer, ResponseTemplate};

        let mock_server = MockServer::start().await;

        Mock::given(method("POST"))
            .and(path("/v1/chat/completions"))
            .respond_with(ResponseTemplate::new(401).set_body_json(serde_json::json!({
                "error": {
                    "message": "Incorrect API key",
                    "type": "invalid_request_error",
                    "code": "invalid_api_key"
                }
            })))
            .expect(1)
            .mount(&mock_server)
            .await;

        let result = extract_title(
            "some page text...",
            "gpt-4o-mini",
            "bad-key",
            &mock_server.uri(),
        )
        .await;

        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(
            matches!(err, crate::error::Error::Llm(_)),
            "expected Error::Llm, got: {err:?}"
        );
    }
}
