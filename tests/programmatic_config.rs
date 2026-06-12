use ocr_cli::config::Config;

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
