use std::path::PathBuf;
use std::sync::Mutex;

use ocr_cli::config::Config;

/// Serialize tests that mutate process-wide environment variables.
static ENV_MUTEX: Mutex<()> = Mutex::new(());

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

// --- PDFIUM_PATH env var tests (serialized via ENV_MUTEX) ---

#[test]
fn builder_pdfium_path_from_env() {
    let _lock = ENV_MUTEX.lock().unwrap();
    let prev = std::env::var("PDFIUM_PATH").ok();

    // SAFETY: env-var mutations are serialized by ENV_MUTEX; no other thread
    // reads PDFIUM_PATH concurrently.
    unsafe { std::env::set_var("PDFIUM_PATH", "/usr/lib/libpdfium.so") };
    let config = Config::builder("sk-m", "sk-o")
        .vault_path("/v")
        .papers_path("/p")
        .build()
        .unwrap();

    // Restore previous value before asserting (in case of panic, the mutex
    // still protects us from concurrent access).
    match &prev {
        Some(v) => unsafe { std::env::set_var("PDFIUM_PATH", v) },
        None => unsafe { std::env::remove_var("PDFIUM_PATH") },
    }

    assert_eq!(
        config.pdfium_path,
        PathBuf::from("/usr/lib/libpdfium.so"),
        "builder should use PDFIUM_PATH env var when .pdfium_path() is not called"
    );
}

#[test]
fn builder_explicit_pdfium_path_overrides_env() {
    let _lock = ENV_MUTEX.lock().unwrap();
    let prev = std::env::var("PDFIUM_PATH").ok();

    // SAFETY: serialized by ENV_MUTEX.
    unsafe { std::env::set_var("PDFIUM_PATH", "/env/path/libpdfium.so") };
    let config = Config::builder("sk-m", "sk-o")
        .vault_path("/v")
        .papers_path("/p")
        .pdfium_path("/explicit/path/libpdfium.so")
        .build()
        .unwrap();

    match &prev {
        Some(v) => unsafe { std::env::set_var("PDFIUM_PATH", v) },
        None => unsafe { std::env::remove_var("PDFIUM_PATH") },
    }

    assert_eq!(
        config.pdfium_path,
        PathBuf::from("/explicit/path/libpdfium.so"),
        "explicit .pdfium_path() must override PDFIUM_PATH env var"
    );
}

#[test]
fn builder_empty_pdfium_env_uses_default() {
    let _lock = ENV_MUTEX.lock().unwrap();
    let prev = std::env::var("PDFIUM_PATH").ok();

    // SAFETY: serialized by ENV_MUTEX.
    unsafe { std::env::set_var("PDFIUM_PATH", "") };
    let config = Config::builder("sk-m", "sk-o")
        .vault_path("/v")
        .papers_path("/p")
        .build()
        .unwrap();

    match &prev {
        Some(v) => unsafe { std::env::set_var("PDFIUM_PATH", v) },
        None => unsafe { std::env::remove_var("PDFIUM_PATH") },
    }

    assert_eq!(
        config.pdfium_path,
        PathBuf::from("/opt/homebrew/lib/libpdfium.dylib"),
        "empty PDFIUM_PATH env var should be treated as absent, falling back to default"
    );
}
