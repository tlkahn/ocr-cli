// Unified error type for ocr-cli.

use thiserror::Error;

#[derive(Debug, Error)]
pub enum Error {
    #[error("PDF error: {0}")]
    Pdf(#[from] lmpdf::Error),

    #[error("LLM error: {0}")]
    Llm(#[from] llm_core::LlmError),

    #[error("HTTP error: {0}")]
    Http(#[from] reqwest::Error),

    #[error("image error: {0}")]
    Image(#[from] image::ImageError),

    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),

    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    #[error("base64 decode error: {0}")]
    Base64(#[from] base64::DecodeError),

    #[error("Mistral API error (status {status}): {body}")]
    MistralApi { status: u16, body: String },

    #[error("truncation error: {0}")]
    Truncation(String),

    #[error("config error: {0}")]
    Config(String),
}

pub type Result<T> = std::result::Result<T, Error>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn error_variants_exist() {
        // Construct each variant to prove they exist with the right shape.
        let _pdf = Error::Pdf(lmpdf::Error::Library(
            lmpdf::error::LibraryError::InitFailed,
        ));
        let _llm = Error::Llm(llm_core::LlmError::Model("x".into()));
        // reqwest::Error cannot be constructed directly; skipped here.
        // image::ImageError tested via From in a later cycle.
        let json_err = serde_json::from_str::<()>("x").unwrap_err();
        let _json = Error::Json(json_err);
        let _io = Error::Io(std::io::Error::new(std::io::ErrorKind::NotFound, "gone"));
        let _b64 = Error::Base64(base64::DecodeError::InvalidPadding);
        let _api = Error::MistralApi {
            status: 400,
            body: "bad request".into(),
        };
        let _trunc = Error::Truncation("too many".into());
        let _cfg = Error::Config("missing key".into());
    }

    #[test]
    fn error_display_non_empty() {
        let cases: Vec<Error> = vec![
            Error::Pdf(lmpdf::Error::Library(
                lmpdf::error::LibraryError::InitFailed,
            )),
            Error::Llm(llm_core::LlmError::Model("x".into())),
            Error::Json(serde_json::from_str::<()>("x").unwrap_err()),
            Error::Io(std::io::Error::new(std::io::ErrorKind::NotFound, "gone")),
            Error::Base64(base64::DecodeError::InvalidPadding),
            Error::MistralApi {
                status: 400,
                body: "bad".into(),
            },
            Error::Truncation("too many".into()),
            Error::Config("missing".into()),
        ];
        for e in &cases {
            assert!(
                !e.to_string().is_empty(),
                "Display should be non-empty for {e:?}"
            );
        }
    }

    #[test]
    fn error_is_debug() {
        let e = Error::Config("test".into());
        let _ = format!("{e:?}");
    }

    #[test]
    fn error_from_lmpdf() {
        let e: Error = lmpdf::Error::Library(lmpdf::error::LibraryError::InitFailed).into();
        assert!(matches!(e, Error::Pdf(_)));
    }

    #[test]
    fn error_from_llm() {
        let e: Error = llm_core::LlmError::Model("test".into()).into();
        assert!(matches!(e, Error::Llm(_)));
    }

    #[test]
    fn error_from_serde_json() {
        let json_err = serde_json::from_str::<()>("invalid").unwrap_err();
        let e: Error = json_err.into();
        assert!(matches!(e, Error::Json(_)));
    }

    #[test]
    fn error_from_io() {
        let io_err = std::io::Error::new(std::io::ErrorKind::NotFound, "gone");
        let e: Error = io_err.into();
        assert!(matches!(e, Error::Io(_)));
        assert!(e.to_string().contains("gone"));
    }

    #[test]
    fn error_from_base64() {
        let e: Error = base64::DecodeError::InvalidPadding.into();
        assert!(matches!(e, Error::Base64(_)));
    }

    #[test]
    fn error_from_image() {
        let img_err = image::load_from_memory(&[0xFF]).unwrap_err();
        let e: Error = img_err.into();
        assert!(matches!(e, Error::Image(_)));
    }

    #[test]
    fn error_mistral_api_display() {
        let e = Error::MistralApi {
            status: 401,
            body: "unauthorized".into(),
        };
        assert!(e.to_string().contains("401"));
        assert!(e.to_string().contains("unauthorized"));
    }

    #[test]
    fn error_truncation_display() {
        let e = Error::Truncation("lead + trail >= page_count".into());
        assert!(e.to_string().contains("lead + trail"));
    }

    #[test]
    fn error_config_display() {
        let e = Error::Config("MISTRAL_API_KEY not set".into());
        assert!(e.to_string().contains("MISTRAL_API_KEY"));
    }

    #[test]
    fn error_is_send_sync() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<Error>();
    }

    #[test]
    fn result_alias_works() {
        let ok: Result<i32> = Ok(42);
        assert_eq!(ok.unwrap(), 42);
        let err: Result<i32> = Err(Error::Config("bad".into()));
        assert!(err.is_err());
    }

    #[test]
    fn error_implements_std_error() {
        fn assert_error<E: std::error::Error>() {}
        assert_error::<Error>();
    }
}
