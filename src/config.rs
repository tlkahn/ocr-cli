use std::fmt;
use std::path::PathBuf;

use crate::cli::Cli;
use crate::error::{Error, Result};

/// Resolved configuration for the OCR pipeline.
#[non_exhaustive]
#[derive(Clone)]
pub struct Config {
    pub mistral_api_key: String,
    pub openai_api_key: String,
    pub model: String,
    pub vault_path: PathBuf,
    pub papers_path: PathBuf,
    pub pdfium_path: PathBuf,
    pub openai_base_url: String,
    pub mistral_base_url: String,
}

impl fmt::Debug for Config {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Config")
            .field("mistral_api_key", &"[REDACTED]")
            .field("openai_api_key", &"[REDACTED]")
            .field("model", &self.model)
            .field("vault_path", &self.vault_path)
            .field("papers_path", &self.papers_path)
            .field("pdfium_path", &self.pdfium_path)
            .field("openai_base_url", &self.openai_base_url)
            .field("mistral_base_url", &self.mistral_base_url)
            .finish()
    }
}

/// Overrides for library consumers who don't have a `Cli`.
#[derive(Debug, Clone, Default)]
pub struct ConfigOverrides {
    pub vault_path: Option<PathBuf>,
    pub papers_path: Option<PathBuf>,
    pub model: Option<String>,
}

impl From<&Cli> for ConfigOverrides {
    fn from(cli: &Cli) -> Self {
        ConfigOverrides {
            vault_path: cli.vault.clone(),
            papers_path: cli.papers.clone(),
            model: cli.model.clone(),
        }
    }
}

/// Default values for optional CLI flags.
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

    /// Resolve configuration from environment variables with explicit overrides.
    /// Same resolution order as `resolve` but accepts `ConfigOverrides` instead of `Cli`.
    pub fn from_env(overrides: &ConfigOverrides) -> Result<Self> {
        Self::from_env_with(overrides, |name| std::env::var(name).ok())
    }

    /// Testable core: accepts a closure for env var lookups.
    fn resolve_with(cli: &Cli, env: impl Fn(&str) -> Option<String>) -> Result<Self> {
        Self::from_env_with(&ConfigOverrides::from(cli), env)
    }

    fn from_env_with(
        overrides: &ConfigOverrides,
        env: impl Fn(&str) -> Option<String>,
    ) -> Result<Self> {
        Self::resolve_inner(
            overrides
                .vault_path
                .clone()
                .map(|p| p.to_string_lossy().into_owned()),
            overrides
                .papers_path
                .clone()
                .map(|p| p.to_string_lossy().into_owned()),
            overrides.model.clone(),
            env,
        )
    }

    fn resolve_inner(
        vault_override: Option<String>,
        papers_override: Option<String>,
        model_override: Option<String>,
        env: impl Fn(&str) -> Option<String>,
    ) -> Result<Self> {
        let mistral_api_key = env_non_empty(&env, "MISTRAL_API_KEY")
            .ok_or_else(|| Error::Config("MISTRAL_API_KEY not set".into()))?;
        let openai_api_key = env_non_empty(&env, "OPENAI_API_KEY")
            .ok_or_else(|| Error::Config("OPENAI_API_KEY not set".into()))?;

        let home = env_non_empty(&env, "HOME");
        let home_ref = home.as_deref();

        let vault_raw = vault_override
            .or_else(|| env_non_empty(&env, "OCR_VAULT_PATH"))
            .unwrap_or_else(|| DEFAULT_VAULT.to_string());
        let vault_path = expand_tilde(std::path::Path::new(&vault_raw), home_ref);

        let papers_raw = papers_override
            .or_else(|| env_non_empty(&env, "OCR_PAPERS_PATH"))
            .unwrap_or_else(|| DEFAULT_PAPERS.to_string());
        let papers_path = expand_tilde(std::path::Path::new(&papers_raw), home_ref);

        let model = model_override
            .or_else(|| env_non_empty(&env, "LLM_DEFAULT_MODEL"))
            .unwrap_or_else(|| DEFAULT_MODEL.to_string());

        let pdfium_path = env_non_empty(&env, "PDFIUM_PATH")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("/opt/homebrew/lib/libpdfium.dylib"));

        let openai_base_url = env_non_empty(&env, "OPENAI_BASE_URL")
            .unwrap_or_else(|| "https://api.openai.com".to_string());
        let mistral_base_url = env_non_empty(&env, "MISTRAL_BASE_URL")
            .unwrap_or_else(|| "https://api.mistral.ai".to_string());

        Ok(Config {
            mistral_api_key,
            openai_api_key,
            model,
            vault_path,
            papers_path,
            pdfium_path,
            openai_base_url,
            mistral_base_url,
        })
    }

    /// Create a [`ConfigBuilder`] with the two required API keys.
    pub fn builder(
        mistral_api_key: impl Into<String>,
        openai_api_key: impl Into<String>,
    ) -> ConfigBuilder {
        ConfigBuilder {
            mistral_api_key: mistral_api_key.into(),
            openai_api_key: openai_api_key.into(),
            model: None,
            vault_path: None,
            papers_path: None,
            pdfium_path: None,
            openai_base_url: None,
            mistral_base_url: None,
        }
    }

    /// Validate that required fields are non-empty.
    pub fn validate(&self) -> Result<()> {
        if self.mistral_api_key.is_empty() {
            return Err(Error::Config("mistral_api_key is empty".into()));
        }
        if self.openai_api_key.is_empty() {
            return Err(Error::Config("openai_api_key is empty".into()));
        }
        Ok(())
    }
}

/// Builder for [`Config`] that applies defaults and validates before construction.
pub struct ConfigBuilder {
    mistral_api_key: String,
    openai_api_key: String,
    model: Option<String>,
    vault_path: Option<PathBuf>,
    papers_path: Option<PathBuf>,
    pdfium_path: Option<PathBuf>,
    openai_base_url: Option<String>,
    mistral_base_url: Option<String>,
}

impl ConfigBuilder {
    pub fn model(mut self, model: impl Into<String>) -> Self {
        self.model = Some(model.into());
        self
    }

    pub fn vault_path(mut self, path: impl Into<PathBuf>) -> Self {
        self.vault_path = Some(path.into());
        self
    }

    pub fn papers_path(mut self, path: impl Into<PathBuf>) -> Self {
        self.papers_path = Some(path.into());
        self
    }

    pub fn pdfium_path(mut self, path: impl Into<PathBuf>) -> Self {
        self.pdfium_path = Some(path.into());
        self
    }

    pub fn openai_base_url(mut self, url: impl Into<String>) -> Self {
        self.openai_base_url = Some(url.into());
        self
    }

    pub fn mistral_base_url(mut self, url: impl Into<String>) -> Self {
        self.mistral_base_url = Some(url.into());
        self
    }

    /// Build the [`Config`], applying defaults for any unset optional fields.
    pub fn build(self) -> Result<Config> {
        let home = std::env::var("HOME").ok();
        let home_ref = home.as_deref();

        let vault_path = self.vault_path.unwrap_or_else(|| {
            expand_tilde(std::path::Path::new(DEFAULT_VAULT), home_ref)
        });
        let papers_path = self.papers_path.unwrap_or_else(|| {
            expand_tilde(std::path::Path::new(DEFAULT_PAPERS), home_ref)
        });

        let config = Config {
            mistral_api_key: self.mistral_api_key,
            openai_api_key: self.openai_api_key,
            model: self.model.unwrap_or_else(|| DEFAULT_MODEL.to_string()),
            vault_path,
            papers_path,
            pdfium_path: self
                .pdfium_path
                .unwrap_or_else(|| PathBuf::from("/opt/homebrew/lib/libpdfium.dylib")),
            openai_base_url: self
                .openai_base_url
                .unwrap_or_else(|| "https://api.openai.com".to_string()),
            mistral_base_url: self
                .mistral_base_url
                .unwrap_or_else(|| "https://api.mistral.ai".to_string()),
        };
        config.validate()?;
        Ok(config)
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

    /// Base env fixture: provides the three keys every happy-path test needs.
    fn base_env(name: &str) -> Option<String> {
        match name {
            "MISTRAL_API_KEY" => Some("sk-mistral".into()),
            "OPENAI_API_KEY" => Some("sk-openai".into()),
            "HOME" => Some("/fakehome".into()),
            _ => None,
        }
    }

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
                "OCR_VAULT_PATH" => Some("/env/vault".into()),
                _ => base_env(name),
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
                "OCR_VAULT_PATH" => Some("/env/vault".into()),
                _ => base_env(name),
            }
        };
        let config = Config::resolve_with(&cli, env).unwrap();
        assert_eq!(config.vault_path, PathBuf::from("/env/vault"));
    }

    #[test]
    fn test_defaults_when_no_env_vars() {
        let cli = Cli::try_parse_from(["ocr-cli", "test.pdf"]).unwrap();
        let config = Config::resolve_with(&cli, base_env).unwrap();
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
        assert_eq!(config.openai_base_url, "https://api.openai.com");
        assert_eq!(config.mistral_base_url, "https://api.mistral.ai");
    }

    #[test]
    fn test_config_has_base_url_defaults() {
        let cli = Cli::try_parse_from(["ocr-cli", "test.pdf"]).unwrap();
        let config = Config::resolve_with(&cli, base_env).unwrap();
        assert_eq!(config.openai_base_url, "https://api.openai.com");
        assert_eq!(config.mistral_base_url, "https://api.mistral.ai");
    }

    #[test]
    fn test_config_base_url_from_env() {
        let cli = Cli::try_parse_from(["ocr-cli", "test.pdf"]).unwrap();
        let env = |name: &str| -> Option<String> {
            match name {
                "OPENAI_BASE_URL" => Some("https://custom-openai.example.com".into()),
                "MISTRAL_BASE_URL" => Some("https://custom-mistral.example.com".into()),
                _ => base_env(name),
            }
        };
        let config = Config::resolve_with(&cli, env).unwrap();
        assert_eq!(config.openai_base_url, "https://custom-openai.example.com");
        assert_eq!(
            config.mistral_base_url,
            "https://custom-mistral.example.com"
        );
    }

    #[test]
    fn test_env_model_overrides_default() {
        let cli = Cli::try_parse_from(["ocr-cli", "test.pdf"]).unwrap();
        let env = |name: &str| -> Option<String> {
            match name {
                "LLM_DEFAULT_MODEL" => Some("gpt-4o".into()),
                _ => base_env(name),
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
                "LLM_DEFAULT_MODEL" => Some("gpt-4o".into()),
                _ => base_env(name),
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
                "PDFIUM_PATH" => Some("/custom/libpdfium.dylib".into()),
                _ => base_env(name),
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
                "OCR_PAPERS_PATH" => Some("/env/papers".into()),
                _ => base_env(name),
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
                "OCR_PAPERS_PATH" => Some("/env/papers".into()),
                _ => base_env(name),
            }
        };
        let config = Config::resolve_with(&cli, env).unwrap();
        assert_eq!(config.papers_path, PathBuf::from("/custom/papers"));
    }

    #[test]
    fn test_cli_vault_tilde_expanded() {
        let cli = Cli::try_parse_from(["ocr-cli", "--vault", "~/my-vault", "test.pdf"]).unwrap();
        let config = Config::resolve_with(&cli, base_env).unwrap();
        assert_eq!(config.vault_path, PathBuf::from("/fakehome/my-vault"));
    }

    #[test]
    fn test_env_vault_tilde_expanded() {
        let cli = Cli::try_parse_from(["ocr-cli", "test.pdf"]).unwrap();
        let env = |name: &str| -> Option<String> {
            match name {
                "OCR_VAULT_PATH" => Some("~/env-vault".into()),
                _ => base_env(name),
            }
        };
        let config = Config::resolve_with(&cli, env).unwrap();
        assert_eq!(config.vault_path, PathBuf::from("/fakehome/env-vault"));
    }

    #[test]
    fn test_cli_papers_tilde_expanded() {
        let cli = Cli::try_parse_from(["ocr-cli", "--papers", "~/my-papers", "test.pdf"]).unwrap();
        let config = Config::resolve_with(&cli, base_env).unwrap();
        assert_eq!(config.papers_path, PathBuf::from("/fakehome/my-papers"));
    }

    #[test]
    fn test_env_papers_tilde_expanded() {
        let cli = Cli::try_parse_from(["ocr-cli", "test.pdf"]).unwrap();
        let env = |name: &str| -> Option<String> {
            match name {
                "OCR_PAPERS_PATH" => Some("~/env-papers".into()),
                _ => base_env(name),
            }
        };
        let config = Config::resolve_with(&cli, env).unwrap();
        assert_eq!(config.papers_path, PathBuf::from("/fakehome/env-papers"));
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
        let _: fn(&Cli) -> Result<Config> = Config::resolve;
    }

    #[test]
    fn test_config_is_debug_clone() {
        fn assert_debug_clone<T: std::fmt::Debug + Clone>() {}
        assert_debug_clone::<Config>();
    }

    #[test]
    fn test_cli_vault_equal_to_default_overrides_env() {
        let cli =
            Cli::try_parse_from(["ocr-cli", "--vault", "~/Documents/Ekuro/", "test.pdf"]).unwrap();
        let env = |name: &str| -> Option<String> {
            match name {
                "OCR_VAULT_PATH" => Some("/env/vault".into()),
                _ => base_env(name),
            }
        };
        let config = Config::resolve_with(&cli, env).unwrap();
        assert_eq!(
            config.vault_path,
            PathBuf::from("/fakehome/Documents/Ekuro/")
        );
    }

    #[test]
    fn test_cli_papers_equal_to_default_overrides_env() {
        let cli = Cli::try_parse_from(["ocr-cli", "--papers", "~/Documents/Papers/", "test.pdf"])
            .unwrap();
        let env = |name: &str| -> Option<String> {
            match name {
                "OCR_PAPERS_PATH" => Some("/env/papers".into()),
                _ => base_env(name),
            }
        };
        let config = Config::resolve_with(&cli, env).unwrap();
        assert_eq!(
            config.papers_path,
            PathBuf::from("/fakehome/Documents/Papers/")
        );
    }

    #[test]
    fn test_cli_model_equal_to_default_overrides_env() {
        let cli = Cli::try_parse_from(["ocr-cli", "--model", "gpt-4o-mini", "test.pdf"]).unwrap();
        let env = |name: &str| -> Option<String> {
            match name {
                "LLM_DEFAULT_MODEL" => Some("gpt-4o".into()),
                _ => base_env(name),
            }
        };
        let config = Config::resolve_with(&cli, env).unwrap();
        assert_eq!(config.model, "gpt-4o-mini");
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

    // --- from_env tests ---

    #[test]
    fn test_from_env_with_defaults() {
        let config = Config::from_env_with(&ConfigOverrides::default(), base_env).unwrap();
        assert_eq!(config.model, "gpt-4o-mini");
        assert_eq!(
            config.vault_path,
            PathBuf::from("/fakehome/Documents/Ekuro/")
        );
        assert_eq!(
            config.papers_path,
            PathBuf::from("/fakehome/Documents/Papers/")
        );
    }

    #[test]
    fn test_from_env_with_overrides() {
        let overrides = ConfigOverrides {
            vault_path: Some(PathBuf::from("/custom/vault")),
            papers_path: Some(PathBuf::from("/custom/papers")),
            model: Some("gpt-4o".into()),
        };
        let config = Config::from_env_with(&overrides, base_env).unwrap();
        assert_eq!(config.vault_path, PathBuf::from("/custom/vault"));
        assert_eq!(config.papers_path, PathBuf::from("/custom/papers"));
        assert_eq!(config.model, "gpt-4o");
    }

    #[test]
    fn test_from_env_overrides_with_tilde() {
        let overrides = ConfigOverrides {
            vault_path: Some(PathBuf::from("~/my-vault")),
            papers_path: None,
            model: None,
        };
        let config = Config::from_env_with(&overrides, base_env).unwrap();
        assert_eq!(config.vault_path, PathBuf::from("/fakehome/my-vault"));
    }

    #[test]
    fn test_from_env_missing_api_key() {
        let env = |name: &str| -> Option<String> {
            match name {
                "OPENAI_API_KEY" => Some("sk-openai".into()),
                _ => None,
            }
        };
        let result = Config::from_env_with(&ConfigOverrides::default(), env);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("MISTRAL_API_KEY"));
    }

    #[test]
    fn test_from_env_signature_exists() {
        let _: fn(&ConfigOverrides) -> Result<Config> = Config::from_env;
    }

    #[test]
    fn test_config_overrides_from_cli() {
        let cli = Cli::try_parse_from([
            "ocr-cli",
            "--vault",
            "/my/vault",
            "--papers",
            "/my/papers",
            "--model",
            "o3",
            "test.pdf",
        ])
        .unwrap();
        let overrides = ConfigOverrides::from(&cli);
        assert_eq!(overrides.vault_path, Some(PathBuf::from("/my/vault")));
        assert_eq!(overrides.papers_path, Some(PathBuf::from("/my/papers")));
        assert_eq!(overrides.model, Some("o3".into()));
    }

    #[test]
    fn test_config_overrides_from_cli_defaults_are_none() {
        let cli = Cli::try_parse_from(["ocr-cli", "test.pdf"]).unwrap();
        let overrides = ConfigOverrides::from(&cli);
        assert!(overrides.vault_path.is_none());
        assert!(overrides.papers_path.is_none());
        assert!(overrides.model.is_none());
    }

    #[test]
    fn test_resolve_with_matches_from_env_with_via_overrides() {
        let cli = Cli::try_parse_from([
            "ocr-cli", "--vault", "/v", "--papers", "/p", "--model", "m", "test.pdf",
        ])
        .unwrap();
        let via_cli = Config::resolve_with(&cli, base_env).unwrap();
        let via_overrides = Config::from_env_with(&ConfigOverrides::from(&cli), base_env).unwrap();
        assert_eq!(via_cli.vault_path, via_overrides.vault_path);
        assert_eq!(via_cli.papers_path, via_overrides.papers_path);
        assert_eq!(via_cli.model, via_overrides.model);
        assert_eq!(via_cli.mistral_api_key, via_overrides.mistral_api_key);
        assert_eq!(via_cli.openai_api_key, via_overrides.openai_api_key);
        assert_eq!(via_cli.pdfium_path, via_overrides.pdfium_path);
        assert_eq!(via_cli.openai_base_url, via_overrides.openai_base_url);
        assert_eq!(via_cli.mistral_base_url, via_overrides.mistral_base_url);
    }

    // --- builder tests ---

    #[test]
    fn test_builder_defaults() {
        let config = Config::builder("sk-mistral", "sk-openai")
            .vault_path("/explicit/vault")
            .papers_path("/explicit/papers")
            .build()
            .unwrap();
        assert_eq!(config.mistral_api_key, "sk-mistral");
        assert_eq!(config.openai_api_key, "sk-openai");
        assert_eq!(config.model, "gpt-4o-mini");
        assert_eq!(config.vault_path, PathBuf::from("/explicit/vault"));
        assert_eq!(config.papers_path, PathBuf::from("/explicit/papers"));
        assert_eq!(
            config.pdfium_path,
            PathBuf::from("/opt/homebrew/lib/libpdfium.dylib")
        );
        assert_eq!(config.openai_base_url, "https://api.openai.com");
        assert_eq!(config.mistral_base_url, "https://api.mistral.ai");
    }

    #[test]
    fn test_builder_all_overrides() {
        let config = Config::builder("sk-m", "sk-o")
            .model("gpt-4o")
            .vault_path("/v")
            .papers_path("/p")
            .pdfium_path("/lib/pdfium.so")
            .openai_base_url("https://custom-openai.example.com")
            .mistral_base_url("https://custom-mistral.example.com")
            .build()
            .unwrap();
        assert_eq!(config.model, "gpt-4o");
        assert_eq!(config.vault_path, PathBuf::from("/v"));
        assert_eq!(config.papers_path, PathBuf::from("/p"));
        assert_eq!(config.pdfium_path, PathBuf::from("/lib/pdfium.so"));
        assert_eq!(config.openai_base_url, "https://custom-openai.example.com");
        assert_eq!(
            config.mistral_base_url,
            "https://custom-mistral.example.com"
        );
    }

    #[test]
    fn test_builder_rejects_empty_mistral_key() {
        let result = Config::builder("", "sk-openai")
            .vault_path("/v")
            .build();
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(matches!(err, Error::Config(_)));
        assert!(err.to_string().contains("mistral_api_key"));
    }

    #[test]
    fn test_builder_rejects_empty_openai_key() {
        let result = Config::builder("sk-mistral", "")
            .vault_path("/v")
            .build();
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(matches!(err, Error::Config(_)));
        assert!(err.to_string().contains("openai_api_key"));
    }

    #[test]
    fn test_validate_empty_keys() {
        let config = Config {
            mistral_api_key: String::new(),
            openai_api_key: "sk-openai".into(),
            model: "gpt-4o-mini".into(),
            vault_path: PathBuf::from("/v"),
            papers_path: PathBuf::from("/p"),
            pdfium_path: PathBuf::from("/lib"),
            openai_base_url: "https://api.openai.com".into(),
            mistral_base_url: "https://api.mistral.ai".into(),
        };
        let err = config.validate().unwrap_err();
        assert!(err.to_string().contains("mistral_api_key"));

        let config2 = Config {
            mistral_api_key: "sk-mistral".into(),
            openai_api_key: String::new(),
            ..config
        };
        let err2 = config2.validate().unwrap_err();
        assert!(err2.to_string().contains("openai_api_key"));
    }

    #[test]
    fn test_debug_redacts_keys() {
        let config = Config {
            mistral_api_key: "super-secret-mistral-key".into(),
            openai_api_key: "super-secret-openai-key".into(),
            model: "gpt-4o-mini".into(),
            vault_path: PathBuf::from("/vault"),
            papers_path: PathBuf::from("/papers"),
            pdfium_path: PathBuf::from("/lib/pdfium.dylib"),
            openai_base_url: "https://api.openai.com".into(),
            mistral_base_url: "https://api.mistral.ai".into(),
        };
        let debug_output = format!("{config:?}");
        assert!(debug_output.contains("[REDACTED]"));
        assert!(!debug_output.contains("super-secret-mistral-key"));
        assert!(!debug_output.contains("super-secret-openai-key"));
    }

    #[test]
    fn test_debug_shows_non_sensitive_fields() {
        let config = Config {
            mistral_api_key: "sk-m".into(),
            openai_api_key: "sk-o".into(),
            model: "gpt-4o-mini".into(),
            vault_path: PathBuf::from("/my/vault"),
            papers_path: PathBuf::from("/my/papers"),
            pdfium_path: PathBuf::from("/lib/pdfium.dylib"),
            openai_base_url: "https://api.openai.com".into(),
            mistral_base_url: "https://api.mistral.ai".into(),
        };
        let debug_output = format!("{config:?}");
        assert!(debug_output.contains("gpt-4o-mini"));
        assert!(debug_output.contains("/my/vault"));
        assert!(debug_output.contains("/my/papers"));
        assert!(debug_output.contains("api.openai.com"));
        assert!(debug_output.contains("api.mistral.ai"));
    }
}
