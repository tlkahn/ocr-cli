// Resolved configuration for the OCR pipeline.

use std::path::PathBuf;

use crate::cli::Cli;
use crate::error::{Error, Result};

/// Resolved configuration for the OCR pipeline.
#[derive(Debug, Clone)]
pub struct Config {
    pub mistral_api_key: String,
    pub openai_api_key: String,
    pub model: String,
    pub vault_path: PathBuf,
    pub papers_path: PathBuf,
    pub pdfium_path: PathBuf,
}

/// Default sentinel values matching clap defaults in `Cli`.
const DEFAULT_VAULT: &str = "~/Documents/Ekuro/";
const DEFAULT_PAPERS: &str = "~/Documents/Papers/";
const DEFAULT_MODEL: &str = "gpt-4o-mini";

/// Look up an env var via the closure, treating empty strings as absent.
fn env_non_empty(env: &impl Fn(&str) -> Option<String>, name: &str) -> Option<String> {
    env(name).filter(|v| !v.is_empty())
}

impl Config {
    /// Resolve configuration from CLI flags and environment variables.
    /// Resolution order: CLI flag -> env var -> default (for optional fields).
    /// Required: MISTRAL_API_KEY, OPENAI_API_KEY (Error::Config if missing).
    pub fn resolve(cli: &Cli) -> Result<Self> {
        Self::resolve_with(cli, |name| std::env::var(name).ok())
    }

    /// Testable core: accepts a closure for env var lookups.
    fn resolve_with(cli: &Cli, env: impl Fn(&str) -> Option<String>) -> Result<Self> {
        let mistral_api_key = env_non_empty(&env, "MISTRAL_API_KEY")
            .ok_or_else(|| Error::Config("MISTRAL_API_KEY not set".into()))?;
        let openai_api_key = env_non_empty(&env, "OPENAI_API_KEY")
            .ok_or_else(|| Error::Config("OPENAI_API_KEY not set".into()))?;

        let home = env_non_empty(&env, "HOME");
        let home_ref = home.as_deref();

        // Vault path: CLI flag -> env var -> default (with tilde expansion).
        let vault_path = if cli.vault != PathBuf::from(DEFAULT_VAULT) {
            cli.vault.clone()
        } else if let Some(val) = env_non_empty(&env, "OCR_VAULT_PATH") {
            PathBuf::from(val)
        } else {
            expand_tilde(std::path::Path::new(DEFAULT_VAULT), home_ref)
        };

        // Papers path: CLI flag -> env var -> default (with tilde expansion).
        let papers_path = if cli.papers != PathBuf::from(DEFAULT_PAPERS) {
            cli.papers.clone()
        } else if let Some(val) = env_non_empty(&env, "OCR_PAPERS_PATH") {
            PathBuf::from(val)
        } else {
            expand_tilde(std::path::Path::new(DEFAULT_PAPERS), home_ref)
        };

        // Model: CLI flag -> env var -> default.
        let model = if cli.model != DEFAULT_MODEL {
            cli.model.clone()
        } else if let Some(val) = env_non_empty(&env, "LLM_DEFAULT_MODEL") {
            val
        } else {
            DEFAULT_MODEL.to_string()
        };

        // Pdfium path: env var -> default.
        let pdfium_path = env_non_empty(&env, "PDFIUM_PATH")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("/opt/homebrew/lib/libpdfium.dylib"));

        Ok(Config {
            mistral_api_key,
            openai_api_key,
            model,
            vault_path,
            papers_path,
            pdfium_path,
        })
    }
}

/// Expand a leading `~` in a path to the value of `$HOME`.
fn expand_tilde(path: &std::path::Path, home: Option<&str>) -> PathBuf {
    let s = path.to_string_lossy();
    if s == "~" {
        match home {
            Some(h) => PathBuf::from(h),
            None => path.to_path_buf(),
        }
    } else if let Some(rest) = s.strip_prefix("~/") {
        match home {
            Some(h) => PathBuf::from(h).join(rest),
            None => path.to_path_buf(),
        }
    } else {
        path.to_path_buf()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;

    #[test]
    fn test_missing_mistral_key_returns_config_error() {
        let cli = Cli::try_parse_from(["ocr-cli", "test.pdf"]).unwrap();
        let env = |name: &str| -> Option<String> {
            match name {
                "OPENAI_API_KEY" => Some("sk-openai".into()),
                _ => None,
            }
        };
        let result = Config::resolve_with(&cli, env);
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(matches!(err, Error::Config(_)));
        assert!(err.to_string().contains("MISTRAL_API_KEY"));
    }

    #[test]
    fn test_missing_openai_key_returns_config_error() {
        let cli = Cli::try_parse_from(["ocr-cli", "test.pdf"]).unwrap();
        let env = |name: &str| -> Option<String> {
            match name {
                "MISTRAL_API_KEY" => Some("sk-mistral".into()),
                _ => None,
            }
        };
        let result = Config::resolve_with(&cli, env);
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(matches!(err, Error::Config(_)));
        assert!(err.to_string().contains("OPENAI_API_KEY"));
    }

    #[test]
    fn test_cli_vault_overrides_env() {
        let cli = Cli::try_parse_from(["ocr-cli", "--vault", "/custom/vault", "test.pdf"]).unwrap();
        let env = |name: &str| -> Option<String> {
            match name {
                "MISTRAL_API_KEY" => Some("sk-mistral".into()),
                "OPENAI_API_KEY" => Some("sk-openai".into()),
                "OCR_VAULT_PATH" => Some("/env/vault".into()),
                _ => None,
            }
        };
        let config = Config::resolve_with(&cli, env).unwrap();
        assert_eq!(config.vault_path, PathBuf::from("/custom/vault"));
    }

    #[test]
    fn test_env_vault_overrides_default() {
        let cli = Cli::try_parse_from(["ocr-cli", "test.pdf"]).unwrap();
        let env = |name: &str| -> Option<String> {
            match name {
                "MISTRAL_API_KEY" => Some("sk-mistral".into()),
                "OPENAI_API_KEY" => Some("sk-openai".into()),
                "OCR_VAULT_PATH" => Some("/env/vault".into()),
                _ => None,
            }
        };
        let config = Config::resolve_with(&cli, env).unwrap();
        assert_eq!(config.vault_path, PathBuf::from("/env/vault"));
    }

    #[test]
    fn test_defaults_when_no_env_vars() {
        let cli = Cli::try_parse_from(["ocr-cli", "test.pdf"]).unwrap();
        let env = |name: &str| -> Option<String> {
            match name {
                "MISTRAL_API_KEY" => Some("sk-mistral".into()),
                "OPENAI_API_KEY" => Some("sk-openai".into()),
                "HOME" => Some("/fakehome".into()),
                _ => None,
            }
        };
        let config = Config::resolve_with(&cli, env).unwrap();
        assert_eq!(config.model, "gpt-4o-mini");
        assert_eq!(
            config.vault_path,
            PathBuf::from("/fakehome/Documents/Ekuro/")
        );
        assert_eq!(
            config.papers_path,
            PathBuf::from("/fakehome/Documents/Papers/")
        );
        assert_eq!(
            config.pdfium_path,
            PathBuf::from("/opt/homebrew/lib/libpdfium.dylib")
        );
    }

    #[test]
    fn test_env_model_overrides_default() {
        let cli = Cli::try_parse_from(["ocr-cli", "test.pdf"]).unwrap();
        let env = |name: &str| -> Option<String> {
            match name {
                "MISTRAL_API_KEY" => Some("sk-mistral".into()),
                "OPENAI_API_KEY" => Some("sk-openai".into()),
                "LLM_DEFAULT_MODEL" => Some("gpt-4o".into()),
                "HOME" => Some("/fakehome".into()),
                _ => None,
            }
        };
        let config = Config::resolve_with(&cli, env).unwrap();
        assert_eq!(config.model, "gpt-4o");
    }

    #[test]
    fn test_cli_model_overrides_env() {
        let cli = Cli::try_parse_from(["ocr-cli", "--model", "o3", "test.pdf"]).unwrap();
        let env = |name: &str| -> Option<String> {
            match name {
                "MISTRAL_API_KEY" => Some("sk-mistral".into()),
                "OPENAI_API_KEY" => Some("sk-openai".into()),
                "LLM_DEFAULT_MODEL" => Some("gpt-4o".into()),
                "HOME" => Some("/fakehome".into()),
                _ => None,
            }
        };
        let config = Config::resolve_with(&cli, env).unwrap();
        assert_eq!(config.model, "o3");
    }

    #[test]
    fn test_pdfium_path_from_env() {
        let cli = Cli::try_parse_from(["ocr-cli", "test.pdf"]).unwrap();
        let env = |name: &str| -> Option<String> {
            match name {
                "MISTRAL_API_KEY" => Some("sk-mistral".into()),
                "OPENAI_API_KEY" => Some("sk-openai".into()),
                "PDFIUM_PATH" => Some("/custom/libpdfium.dylib".into()),
                "HOME" => Some("/fakehome".into()),
                _ => None,
            }
        };
        let config = Config::resolve_with(&cli, env).unwrap();
        assert_eq!(config.pdfium_path, PathBuf::from("/custom/libpdfium.dylib"));
    }

    #[test]
    fn test_env_papers_overrides_default() {
        let cli = Cli::try_parse_from(["ocr-cli", "test.pdf"]).unwrap();
        let env = |name: &str| -> Option<String> {
            match name {
                "MISTRAL_API_KEY" => Some("sk-mistral".into()),
                "OPENAI_API_KEY" => Some("sk-openai".into()),
                "OCR_PAPERS_PATH" => Some("/env/papers".into()),
                "HOME" => Some("/fakehome".into()),
                _ => None,
            }
        };
        let config = Config::resolve_with(&cli, env).unwrap();
        assert_eq!(config.papers_path, PathBuf::from("/env/papers"));
    }

    #[test]
    fn test_cli_papers_overrides_env() {
        let cli =
            Cli::try_parse_from(["ocr-cli", "--papers", "/custom/papers", "test.pdf"]).unwrap();
        let env = |name: &str| -> Option<String> {
            match name {
                "MISTRAL_API_KEY" => Some("sk-mistral".into()),
                "OPENAI_API_KEY" => Some("sk-openai".into()),
                "OCR_PAPERS_PATH" => Some("/env/papers".into()),
                "HOME" => Some("/fakehome".into()),
                _ => None,
            }
        };
        let config = Config::resolve_with(&cli, env).unwrap();
        assert_eq!(config.papers_path, PathBuf::from("/custom/papers"));
    }

    #[test]
    fn test_expand_tilde_with_home() {
        assert_eq!(
            expand_tilde(std::path::Path::new("~/foo/bar"), Some("/home/user")),
            PathBuf::from("/home/user/foo/bar")
        );
    }

    #[test]
    fn test_expand_tilde_no_tilde() {
        assert_eq!(
            expand_tilde(std::path::Path::new("/absolute/path"), Some("/home/user")),
            PathBuf::from("/absolute/path")
        );
    }

    #[test]
    fn test_expand_tilde_bare_tilde() {
        assert_eq!(
            expand_tilde(std::path::Path::new("~"), Some("/home/user")),
            PathBuf::from("/home/user")
        );
    }

    #[test]
    fn test_expand_tilde_no_home() {
        assert_eq!(
            expand_tilde(std::path::Path::new("~/foo"), None),
            PathBuf::from("~/foo")
        );
    }

    #[test]
    fn test_resolve_signature_exists() {
        // Compile-check only: verify the public API signature exists.
        let _: fn(&Cli) -> Result<Config> = Config::resolve;
    }

    #[test]
    fn test_config_is_debug_clone() {
        fn assert_debug_clone<T: std::fmt::Debug + Clone>() {}
        assert_debug_clone::<Config>();
    }

    #[test]
    fn test_empty_env_var_treated_as_absent() {
        let cli = Cli::try_parse_from(["ocr-cli", "test.pdf"]).unwrap();
        let env = |name: &str| -> Option<String> {
            match name {
                "MISTRAL_API_KEY" => Some("".into()),
                "OPENAI_API_KEY" => Some("sk-openai".into()),
                _ => None,
            }
        };
        let result = Config::resolve_with(&cli, env);
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(matches!(err, Error::Config(_)));
        assert!(err.to_string().contains("MISTRAL_API_KEY"));
    }
}
