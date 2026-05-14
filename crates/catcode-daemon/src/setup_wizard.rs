//! First-run setup wizard for CatCode.
//!
//! Detects missing configuration and guides the user through setup.
//! Auto-triggered on first launch.

use std::path::PathBuf;
use std::fs;

/// Result of configuration detection.
#[derive(Debug)]
pub struct ConfigStatus {
    /// Whether a config file exists already.
    pub config_exists: bool,
    /// Path to the config file.
    pub config_path: PathBuf,
    /// Detected API keys (from env vars).
    pub detected_keys: Vec<DetectedKey>,
    /// Missing API keys (no env var found).
    pub missing_keys: Vec<ProviderInfo>,
}

#[derive(Debug)]
pub struct DetectedKey {
    pub provider: &'static str,
    pub env_var: &'static str,
}

#[derive(Debug, Clone)]
pub struct ProviderInfo {
    pub name: &'static str,
    pub env_var: &'static str,
    pub docs_url: &'static str,
    pub requires_key: bool,
}

/// Known providers and their env vars.
pub static KNOWN_PROVIDERS: &[ProviderInfo] = &[
    ProviderInfo { name: "DeepSeek", env_var: "DEEPSEEK_API_KEY", docs_url: "https://platform.deepseek.com/api-keys", requires_key: true },
    ProviderInfo { name: "Anthropic", env_var: "ANTHROPIC_API_KEY", docs_url: "https://console.anthropic.com/", requires_key: true },
    ProviderInfo { name: "OpenAI", env_var: "OPENAI_API_KEY", docs_url: "https://platform.openai.com/api-keys", requires_key: true },
    ProviderInfo { name: "Qwen", env_var: "QWEN_API_KEY", docs_url: "https://help.aliyun.com/zh/dashscope", requires_key: true },
    ProviderInfo { name: "Google", env_var: "GOOGLE_API_KEY", docs_url: "https://makersuite.google.com/app/apikey", requires_key: true },
    ProviderInfo { name: "MiniMax", env_var: "MINIMAX_API_KEY", docs_url: "https://platform.minimaxi.com", requires_key: true },
    ProviderInfo { name: "GLM (Zhipu)", env_var: "GLM_API_KEY", docs_url: "https://open.bigmodel.cn/", requires_key: true },
    ProviderInfo { name: "OpenRouter", env_var: "OPENROUTER_API_KEY", docs_url: "https://openrouter.ai/keys", requires_key: true },
    ProviderInfo { name: "Volcengine", env_var: "VOLCENGINE_API_KEY", docs_url: "https://console.volcengine.com/ark", requires_key: true },
    ProviderInfo { name: "Ollama (local)", env_var: "", docs_url: "https://ollama.com", requires_key: false },
];

/// Detect current configuration status.
pub fn detect_config() -> ConfigStatus {
    let config_dir = get_config_dir();
    let config_path = config_dir.join("config.toml");
    let config_exists = config_path.exists();

    let mut detected_keys = Vec::new();
    let mut missing_keys = Vec::new();

    for provider in KNOWN_PROVIDERS {
        if provider.env_var.is_empty() {
            continue;
        }
        if std::env::var(provider.env_var).is_ok() {
            detected_keys.push(DetectedKey {
                provider: provider.name,
                env_var: provider.env_var,
            });
        } else {
            missing_keys.push(provider.clone());
        }
    }

    ConfigStatus { config_exists, config_path, detected_keys, missing_keys }
}

/// Get the config directory path.
pub fn get_config_dir() -> PathBuf {
    let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".to_string());
    PathBuf::from(home).join(".config").join("catcode")
}

/// Get the data directory path.
pub fn get_data_dir() -> PathBuf {
    let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".to_string());
    PathBuf::from(home).join(".local").join("share").join("catcode")
}

/// Generate a default config.toml from detected env vars.
pub fn generate_default_config(detected: &[DetectedKey]) -> String {
    let mut config = String::new();
    config.push_str("[daemon]\n");
    config.push_str("host = \"127.0.0.1\"\n");
    config.push_str("port = 7070\n\n");

    config.push_str("[defaults]\n");
    if let Some(first) = detected.first() {
        let model = match first.provider {
            "DeepSeek" => "deepseek-chat",
            "Anthropic" => "claude-sonnet-4-20250514",
            "OpenAI" => "gpt-4.1",
            "Qwen" => "qwen3",
            _ => "default",
        };
        config.push_str(&format!("provider = \"{}\"\n", first.provider.to_lowercase()));
        config.push_str(&format!("model = \"{}\"\n", model));
    } else {
        config.push_str("provider = \"deepseek\"\n");
        config.push_str("model = \"deepseek-chat\"\n");
    }
    config.push_str("\n[budget]\n");
    config.push_str("session_limit_tokens = 500000\n");
    config.push_str("warning_threshold = 0.80\n");
    config
}

/// Print a colored welcome banner.
pub fn print_welcome(status: &ConfigStatus) {
    let banner = r#"
   ╔══════════════════════════════════════╗
   ║           CatCode v0.1              ║
   ║     AI Coding Agent                 ║
   ╚══════════════════════════════════════╝
"#;
    println!("{}", banner);

    if status.config_exists {
        println!("  Config: {} ✓", status.config_path.display());
    } else {
        println!("  Config: not found — run 'catcode init' to set up");
        println!("  Or just set API keys as env vars and start coding:");
        for provider in KNOWN_PROVIDERS {
            if !provider.env_var.is_empty() {
                println!("    export {}=your-key", provider.env_var);
            }
        }
    }

    println!();
    if !status.detected_keys.is_empty() {
        println!("  Detected API keys:");
        for key in &status.detected_keys {
            println!("    ✓ {}", key.provider);
        }
    }
    println!();
}

/// Ensure the config directory exists.
pub fn ensure_dirs() -> std::io::Result<()> {
    let config_dir = get_config_dir();
    let data_dir = get_data_dir();
    fs::create_dir_all(&config_dir)?;
    fs::create_dir_all(&data_dir)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_get_config_dir() {
        let dir = get_config_dir();
        assert!(dir.ends_with(".config/catcode"));
    }

    #[test]
    fn test_get_data_dir() {
        let dir = get_data_dir();
        assert!(dir.ends_with(".local/share/catcode"));
    }

    #[test]
    fn test_generate_default_config_with_detected() {
        let detected = vec![
            DetectedKey { provider: "DeepSeek", env_var: "DEEPSEEK_API_KEY" },
        ];
        let config = generate_default_config(&detected);
        assert!(config.contains("provider = \"deepseek\""));
        assert!(config.contains("model = \"deepseek-chat\""));
        assert!(config.contains("port = 7070"));
        assert!(config.contains("session_limit_tokens = 500000"));
    }

    #[test]
    fn test_generate_default_config_empty() {
        let config = generate_default_config(&[]);
        assert!(config.contains("provider = \"deepseek\""));
        assert!(config.contains("model = \"deepseek-chat\""));
    }

    #[test]
    fn test_known_providers() {
        assert!(!KNOWN_PROVIDERS.is_empty());
        let deepseek = KNOWN_PROVIDERS.iter().find(|p| p.name == "DeepSeek").unwrap();
        assert_eq!(deepseek.env_var, "DEEPSEEK_API_KEY");
        assert!(deepseek.requires_key);
    }

    #[test]
    fn test_detect_config_no_env() {
        // Run in a clean environment — no API keys set, config won't exist
        let status = detect_config();
        assert!(!status.config_exists);
        // All key-requiring providers should be in missing_keys
        let keyed_count = KNOWN_PROVIDERS.iter().filter(|p| p.requires_key).count();
        assert_eq!(status.missing_keys.len(), keyed_count);
        assert!(status.detected_keys.is_empty());
    }
}
