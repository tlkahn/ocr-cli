// Mistral OCR API client: request/response types and async ocr_pdf() function.

use serde::{Deserialize, Serialize};

// ---------- Request types ----------

/// How to send the document to Mistral OCR.
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type")]
pub enum DocumentSource {
    /// Inline base64 PDF via data URI.
    #[serde(rename = "document_url")]
    Base64Pdf {
        /// Must be "data:application/pdf;base64,<base64-encoded-bytes>"
        document_url: String,
    },
}

impl DocumentSource {
    /// Construct from raw PDF bytes (encodes to base64 data URI).
    pub fn from_pdf_bytes(bytes: &[u8]) -> Self {
        use base64::Engine as _;
        let b64 = base64::engine::general_purpose::STANDARD.encode(bytes);
        DocumentSource::Base64Pdf {
            document_url: format!("data:application/pdf;base64,{b64}"),
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct OcrRequest {
    pub model: String,
    pub document: DocumentSource,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub include_image_base64: Option<bool>,
}

// ---------- Response types ----------

#[derive(Debug, Clone, Deserialize)]
#[allow(dead_code)]
pub struct OcrResponse {
    pub pages: Vec<OcrPage>,
    pub model: String,
    #[serde(default)]
    pub document_annotation: Option<String>,
    pub usage_info: OcrUsageInfo,
}

#[derive(Debug, Clone, Deserialize)]
#[allow(dead_code)]
pub struct OcrPage {
    pub index: u32,
    pub markdown: String,
    #[serde(default)]
    pub images: Vec<OcrImage>,
    #[serde(default)]
    pub dimensions: Option<OcrPageDimensions>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct OcrImage {
    pub id: String,
    pub top_left_x: Option<i64>,
    pub top_left_y: Option<i64>,
    pub bottom_right_x: Option<i64>,
    pub bottom_right_y: Option<i64>,
    #[serde(default)]
    pub image_base64: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[allow(dead_code)]
pub struct OcrPageDimensions {
    pub dpi: u32,
    pub height: u32,
    pub width: u32,
}

#[derive(Debug, Clone, Deserialize)]
#[allow(dead_code)]
pub struct OcrUsageInfo {
    pub pages_processed: u32,
    #[serde(default)]
    pub doc_size_bytes: Option<u64>,
}

// ---------- API client function ----------

/// POST the PDF bytes to Mistral OCR and return the parsed response.
pub async fn ocr_pdf(
    client: &reqwest::Client,
    base_url: &str,
    api_key: &str,
    pdf_bytes: &[u8],
    include_images: bool,
) -> crate::error::Result<OcrResponse> {
    let document = DocumentSource::from_pdf_bytes(pdf_bytes);
    let request = OcrRequest {
        model: "mistral-ocr-latest".into(),
        document,
        include_image_base64: if include_images { Some(true) } else { None },
    };

    let url = format!("{base_url}/v1/ocr");
    let response = client
        .post(&url)
        .bearer_auth(api_key)
        .json(&request)
        .send()
        .await?;

    let status = response.status();
    if !status.is_success() {
        let body = response.text().await.unwrap_or_default();
        return Err(crate::error::Error::MistralApi {
            status: status.as_u16(),
            body,
        });
    }

    let ocr_response: OcrResponse = response.json().await?;
    Ok(ocr_response)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ocr_request_serializes_to_expected_json() {
        let doc = DocumentSource::from_pdf_bytes(&[1, 2, 3]);
        let req = OcrRequest {
            model: "mistral-ocr-latest".into(),
            document: doc,
            include_image_base64: Some(true),
        };
        let json = serde_json::to_value(&req).unwrap();

        assert_eq!(json["model"], "mistral-ocr-latest");
        assert_eq!(json["document"]["type"], "document_url");

        let url = json["document"]["document_url"].as_str().unwrap();
        assert!(url.starts_with("data:application/pdf;base64,"));

        // [1, 2, 3] encodes to "AQID" in standard base64
        assert!(url.ends_with("AQID"));

        assert_eq!(json["include_image_base64"], true);
    }

    fn response_fixture() -> String {
        serde_json::json!({
            "model": "mistral-ocr-latest",
            "pages": [
                {
                    "index": 0,
                    "markdown": "# Hello World\n\nThis is page one.",
                    "images": [
                        {
                            "id": "img_0",
                            "top_left_x": 100,
                            "top_left_y": 200,
                            "bottom_right_x": 400,
                            "bottom_right_y": 500,
                            "image_base64": "iVBORw0KGgo="
                        }
                    ],
                    "dimensions": {
                        "dpi": 200,
                        "height": 792,
                        "width": 612
                    }
                },
                {
                    "index": 1,
                    "markdown": "## Page Two\n\nMore content here.",
                    "images": [],
                    "dimensions": {
                        "dpi": 200,
                        "height": 792,
                        "width": 612
                    }
                }
            ],
            "document_annotation": null,
            "usage_info": {
                "pages_processed": 2,
                "doc_size_bytes": 12345
            }
        })
        .to_string()
    }

    #[test]
    fn test_ocr_response_deserializes_from_fixture() {
        let fixture = response_fixture();
        let resp: OcrResponse = serde_json::from_str(&fixture).unwrap();
        assert_eq!(resp.pages.len(), 2);
        assert_eq!(resp.pages[0].index, 0);
        assert!(resp.pages[0].markdown.contains("Hello World"));
        assert_eq!(resp.pages[0].images.len(), 1);
        assert_eq!(resp.pages[0].images[0].id, "img_0");
        assert_eq!(
            resp.pages[0].images[0].image_base64.as_deref(),
            Some("iVBORw0KGgo=")
        );
        assert_eq!(resp.pages[0].images[0].top_left_x, Some(100));
        assert!(resp.pages[1].images.is_empty());
        assert_eq!(resp.model, "mistral-ocr-latest");
        assert_eq!(resp.usage_info.pages_processed, 2);
        assert_eq!(resp.usage_info.doc_size_bytes, Some(12345));
    }

    #[test]
    fn test_ocr_request_omits_none_include_image() {
        let doc = DocumentSource::from_pdf_bytes(&[1, 2, 3]);
        let req = OcrRequest {
            model: "mistral-ocr-latest".into(),
            document: doc,
            include_image_base64: None,
        };
        let json = serde_json::to_value(&req).unwrap();
        let obj = json.as_object().unwrap();
        assert!(
            !obj.contains_key("include_image_base64"),
            "include_image_base64 should be absent when None"
        );
    }

    // Cycle 4: empty images array deserializes correctly.
    #[test]
    fn test_ocr_response_empty_images_list() {
        let json = serde_json::json!({
            "model": "mistral-ocr-latest",
            "pages": [{
                "index": 0,
                "markdown": "text",
                "images": []
            }],
            "usage_info": { "pages_processed": 1 }
        })
        .to_string();
        let resp: OcrResponse = serde_json::from_str(&json).unwrap();
        assert!(resp.pages[0].images.is_empty());
    }

    // Cycle 5: missing images key defaults to empty vec.
    #[test]
    fn test_ocr_response_missing_images_key() {
        let json = serde_json::json!({
            "model": "mistral-ocr-latest",
            "pages": [{
                "index": 0,
                "markdown": "text"
            }],
            "usage_info": { "pages_processed": 1 }
        })
        .to_string();
        let resp: OcrResponse = serde_json::from_str(&json).unwrap();
        assert!(resp.pages[0].images.is_empty());
    }

    // Cycle 6: image_base64 absent or null both yield None.
    #[test]
    fn test_ocr_image_without_base64() {
        // Absent key
        let json_absent = serde_json::json!({
            "id": "img_1",
            "top_left_x": 0,
            "top_left_y": 0,
            "bottom_right_x": 100,
            "bottom_right_y": 100
        })
        .to_string();
        let img: OcrImage = serde_json::from_str(&json_absent).unwrap();
        assert_eq!(img.image_base64, None);

        // Explicit null
        let json_null = serde_json::json!({
            "id": "img_2",
            "top_left_x": 0,
            "top_left_y": 0,
            "bottom_right_x": 100,
            "bottom_right_y": 100,
            "image_base64": null
        })
        .to_string();
        let img2: OcrImage = serde_json::from_str(&json_null).unwrap();
        assert_eq!(img2.image_base64, None);
    }

    // Cycle 7: null coordinates yield None.
    #[test]
    fn test_ocr_image_with_null_coordinates() {
        let json = serde_json::json!({
            "id": "img_3",
            "top_left_x": null,
            "top_left_y": null,
            "bottom_right_x": null,
            "bottom_right_y": null
        })
        .to_string();
        let img: OcrImage = serde_json::from_str(&json).unwrap();
        assert_eq!(img.top_left_x, None);
        assert_eq!(img.top_left_y, None);
        assert_eq!(img.bottom_right_x, None);
        assert_eq!(img.bottom_right_y, None);
    }

    // Cycle 8: base64 roundtrip.
    #[test]
    fn test_document_source_from_pdf_bytes_roundtrip() {
        use base64::Engine as _;
        let original = b"hello pdf content";
        let doc = DocumentSource::from_pdf_bytes(original);
        let DocumentSource::Base64Pdf { document_url } = &doc;
        let b64_str = document_url
            .strip_prefix("data:application/pdf;base64,")
            .expect("should have data URI prefix");
        let decoded = base64::engine::general_purpose::STANDARD
            .decode(b64_str)
            .unwrap();
        assert_eq!(decoded, original);
    }

    // Cycle 9: ocr_pdf sends correct request and parses response.
    #[tokio::test]
    async fn test_ocr_pdf_sends_correct_request() {
        use wiremock::matchers::{header, method, path};
        use wiremock::{Mock, MockServer, ResponseTemplate};

        let mock_server = MockServer::start().await;
        let fixture = response_fixture();

        Mock::given(method("POST"))
            .and(path("/v1/ocr"))
            .and(header("Authorization", "Bearer test-key"))
            .and(header("Content-Type", "application/json"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_string(&fixture)
                    .insert_header("Content-Type", "application/json"),
            )
            .expect(1)
            .mount(&mock_server)
            .await;

        let client = reqwest::Client::new();
        let resp = ocr_pdf(&client, &mock_server.uri(), "test-key", &[1, 2, 3], true)
            .await
            .unwrap();

        assert_eq!(resp.model, "mistral-ocr-latest");
        assert_eq!(resp.pages.len(), 2);
        assert_eq!(resp.usage_info.pages_processed, 2);
    }

    // Cycle 10: ocr_pdf returns error on 401.
    #[tokio::test]
    async fn test_ocr_pdf_returns_error_on_401() {
        use wiremock::matchers::{method, path};
        use wiremock::{Mock, MockServer, ResponseTemplate};

        let mock_server = MockServer::start().await;

        Mock::given(method("POST"))
            .and(path("/v1/ocr"))
            .respond_with(ResponseTemplate::new(401).set_body_string("unauthorized"))
            .expect(1)
            .mount(&mock_server)
            .await;

        let client = reqwest::Client::new();
        let result = ocr_pdf(&client, &mock_server.uri(), "bad-key", &[1, 2, 3], false).await;

        assert!(result.is_err());
        let err = result.unwrap_err();
        match err {
            crate::error::Error::MistralApi { status, body } => {
                assert_eq!(status, 401);
                assert!(body.contains("unauthorized"));
            }
            other => panic!("expected MistralApi error, got: {other:?}"),
        }
    }

    // Cycle 11: ocr_pdf returns error on 500.
    #[tokio::test]
    async fn test_ocr_pdf_returns_error_on_500() {
        use wiremock::matchers::{method, path};
        use wiremock::{Mock, MockServer, ResponseTemplate};

        let mock_server = MockServer::start().await;

        Mock::given(method("POST"))
            .and(path("/v1/ocr"))
            .respond_with(ResponseTemplate::new(500).set_body_string("internal server error"))
            .expect(1)
            .mount(&mock_server)
            .await;

        let client = reqwest::Client::new();
        let result = ocr_pdf(&client, &mock_server.uri(), "test-key", &[1, 2, 3], false).await;

        assert!(result.is_err());
        let err = result.unwrap_err();
        match err {
            crate::error::Error::MistralApi { status, body } => {
                assert_eq!(status, 500);
                assert!(body.contains("internal server error"));
            }
            other => panic!("expected MistralApi error, got: {other:?}"),
        }
    }
}
