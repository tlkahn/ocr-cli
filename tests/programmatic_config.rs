use std::path::PathBuf;

use ocr_cli::config::Config;

#[test]
fn builder_with_required_fields_applies_defaults() {
    let config = Config::builder("sk-mistral-test", "sk-openai-test")
        .vault_path("/explicit/vault")
        .papers_path("/explicit/papers")
        .build()
        .unwrap();

    assert_eq!(config.mistral_api_key, "sk-mistral-test");
    assert_eq!(config.openai_api_key, "sk-openai-test");
    assert_eq!(config.model, "gpt-4o-mini");
    assert_eq!(config.openai_base_url, "https://api.openai.com");
    assert_eq!(config.mistral_base_url, "https://api.mistral.ai");
}

#[test]
fn builder_with_all_overrides() {
    let config = Config::builder("sk-m", "sk-o")
        .model("o3")
        .vault_path("/v")
        .papers_path("/p")
        .pdfium_path("/lib/pdfium.so")
        .openai_base_url("https://custom.openai.example.com")
        .mistral_base_url("https://custom.mistral.example.com")
        .build()
        .unwrap();

    assert_eq!(config.model, "o3");
    assert_eq!(config.vault_path, PathBuf::from("/v"));
    assert_eq!(config.papers_path, PathBuf::from("/p"));
    assert_eq!(config.pdfium_path, PathBuf::from("/lib/pdfium.so"));
    assert_eq!(
        config.openai_base_url,
        "https://custom.openai.example.com"
    );
    assert_eq!(
        config.mistral_base_url,
        "https://custom.mistral.example.com"
    );
}

#[test]
fn builder_rejects_empty_mistral_key() {
    let err = Config::builder("", "sk-openai")
        .vault_path("/v")
        .build()
        .unwrap_err();
    assert!(err.to_string().contains("mistral_api_key"));
}

#[test]
fn builder_rejects_empty_openai_key() {
    let err = Config::builder("sk-mistral", "")
        .vault_path("/v")
        .build()
        .unwrap_err();
    assert!(err.to_string().contains("openai_api_key"));
}

#[test]
fn validate_returns_error_for_empty_keys() {
    let err = Config::builder("", "sk-o")
        .vault_path("/v")
        .build()
        .unwrap_err();
    assert!(err.to_string().contains("mistral_api_key"));
}

#[test]
fn debug_redacts_api_keys() {
    let config = Config::builder("super-secret-mistral", "super-secret-openai")
        .vault_path("/v")
        .papers_path("/p")
        .build()
        .unwrap();
    let debug = format!("{config:?}");

    assert!(debug.contains("[REDACTED]"));
    assert!(!debug.contains("super-secret-mistral"));
    assert!(!debug.contains("super-secret-openai"));
    assert!(debug.contains("gpt-4o-mini"));
    assert!(debug.contains("/v"));
}

/// Demonstrates the Lit use case: injecting keys from an encrypted keystore.
#[test]
fn lit_keystore_use_case() {
    let keystore_mistral = "decrypted-mistral-key-from-lit";
    let keystore_openai = "decrypted-openai-key-from-lit";

    let config = Config::builder(keystore_mistral, keystore_openai)
        .vault_path("/app/vault")
        .papers_path("/app/papers")
        .model("gpt-4o")
        .build()
        .unwrap();

    assert_eq!(config.mistral_api_key, keystore_mistral);
    assert_eq!(config.openai_api_key, keystore_openai);
    assert_eq!(config.model, "gpt-4o");
}
