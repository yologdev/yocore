//! Configuration management for Yolog Core
//!
//! Loads settings from TOML file at ~/.yolog/config.toml

use crate::error::{CoreError, Result};
use serde::{Deserialize, Serialize};
use std::net::SocketAddr;
use std::path::{Path, PathBuf};

/// Main configuration structure
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    /// Server configuration
    #[serde(default)]
    pub server: ServerConfig,

    /// Watch paths configuration
    #[serde(default)]
    pub watch: Vec<WatchConfig>,

    /// AI feature configuration
    #[serde(default)]
    pub ai: AiConfig,

    /// Data directory (defaults to ~/.yolog)
    #[serde(default = "default_data_dir")]
    pub data_dir: PathBuf,
}

fn default_data_dir() -> PathBuf {
    dirs::home_dir()
        .map(|p| p.join(".yolog"))
        .unwrap_or_else(|| PathBuf::from(".yolog"))
}

/// HTTP server configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerConfig {
    /// Server port (default: 19420)
    #[serde(default = "default_port")]
    pub port: u16,

    /// Server host (default: 127.0.0.1 - localhost only)
    /// WARNING: Setting to "0.0.0.0" exposes the server to your network.
    /// Only do this on trusted networks and consider setting api_key.
    #[serde(default = "default_host")]
    pub host: String,

    /// Optional API key for authentication
    /// Required in Authorization header if set: "Authorization: Bearer <key>"
    #[serde(default)]
    pub api_key: Option<String>,
}

fn default_port() -> u16 {
    19420 // Uncommon port to avoid conflicts
}

fn default_host() -> String {
    "127.0.0.1".to_string() // Localhost only - secure by default
}

impl Default for ServerConfig {
    fn default() -> Self {
        ServerConfig {
            port: default_port(),
            host: default_host(),
            api_key: None,
        }
    }
}

/// Watch path configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WatchConfig {
    /// Path to watch for session files
    pub path: PathBuf,

    /// Parser type (claude_code, openclaw, etc.)
    #[serde(default = "default_parser")]
    pub parser: String,

    /// Whether this watch path is enabled
    #[serde(default = "default_true")]
    pub enabled: bool,
}

fn default_parser() -> String {
    "claude_code".to_string()
}

fn default_true() -> bool {
    true
}

/// AI feature configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AiConfig {
    /// Whether AI features are enabled
    #[serde(default)]
    pub enabled: bool,

    /// AI provider (anthropic, openai, etc.)
    #[serde(default)]
    pub provider: Option<String>,

    /// AI feature toggles
    #[serde(default)]
    pub features: AiFeatures,
}

impl Default for AiConfig {
    fn default() -> Self {
        AiConfig {
            enabled: false,
            provider: None,
            features: AiFeatures::default(),
        }
    }
}

/// Individual AI feature toggles
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AiFeatures {
    /// Generate session titles
    #[serde(default = "default_true")]
    pub title_generation: bool,

    /// Discover skills from sessions
    #[serde(default = "default_true")]
    pub skills_discovery: bool,

    /// Extract memories from sessions
    #[serde(default = "default_true")]
    pub memory_extraction: bool,
}

impl Default for AiFeatures {
    fn default() -> Self {
        AiFeatures {
            title_generation: true,
            skills_discovery: true,
            memory_extraction: true,
        }
    }
}

impl Default for Config {
    fn default() -> Self {
        Config {
            server: ServerConfig::default(),
            watch: vec![],
            ai: AiConfig::default(),
            data_dir: default_data_dir(),
        }
    }
}

impl Config {
    /// Load configuration from a TOML file
    pub fn from_file<P: AsRef<Path>>(path: P) -> Result<Self> {
        let path = path.as_ref();

        // Expand ~ to home directory
        let expanded_path = if path.starts_with("~") {
            if let Some(home) = dirs::home_dir() {
                home.join(path.strip_prefix("~").unwrap())
            } else {
                path.to_path_buf()
            }
        } else {
            path.to_path_buf()
        };

        if !expanded_path.exists() {
            return Err(CoreError::Config(format!(
                "Configuration file not found: {}",
                expanded_path.display()
            )));
        }

        let content = std::fs::read_to_string(&expanded_path)?;
        let config: Config = toml::from_str(&content)?;

        Ok(config)
    }

    /// Load configuration from file or use defaults
    pub fn load_or_default<P: AsRef<Path>>(path: P) -> Self {
        Self::from_file(path).unwrap_or_default()
    }

    /// Get the default configuration file path
    pub fn default_path() -> PathBuf {
        dirs::home_dir()
            .map(|p| p.join(".yolog").join("config.toml"))
            .unwrap_or_else(|| PathBuf::from(".yolog/config.toml"))
    }

    /// Get the data directory, expanding ~ if present
    pub fn data_dir(&self) -> PathBuf {
        expand_path(&self.data_dir)
    }

    /// Get the server socket address
    pub fn server_addr(&self) -> SocketAddr {
        use std::net::ToSocketAddrs;

        format!("{}:{}", self.server.host, self.server.port)
            .to_socket_addrs()
            .ok()
            .and_then(|mut addrs| addrs.next())
            .unwrap_or_else(|| SocketAddr::from(([127, 0, 0, 1], self.server.port)))
    }

    /// Get expanded watch paths
    pub fn watch_paths(&self) -> Vec<(PathBuf, String)> {
        self.watch
            .iter()
            .filter(|w| w.enabled)
            .map(|w| (expand_path(&w.path), w.parser.clone()))
            .collect()
    }

    /// Create a default configuration file at the given path
    pub fn create_default<P: AsRef<Path>>(path: P) -> Result<()> {
        // Write a well-commented config file
        let content = r#"# Yolog Core Configuration

[server]
# Port to listen on (default: 19420)
port = 19420

# Host to bind to
# "127.0.0.1" = localhost only (secure, recommended)
# "0.0.0.0" = all interfaces (exposes to network - use with api_key!)
host = "127.0.0.1"

# Optional API key for authentication
# If set, clients must send: Authorization: Bearer <api_key>
# api_key = "your-secret-key"

# Watch paths for session files
[[watch]]
path = "~/.claude/projects"
parser = "claude_code"
enabled = true

# Add more watch paths as needed:
# [[watch]]
# path = "~/.openclaw/workspace"
# parser = "openclaw"
# enabled = true

[ai]
# Enable AI features (title generation, memory extraction, etc.)
enabled = false
# provider = "anthropic"

[ai.features]
title_generation = true
skills_discovery = true
memory_extraction = true
"#;

        let path = path.as_ref();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(path, content)?;

        Ok(())
    }
}

/// Expand ~ to home directory in paths
fn expand_path(path: &Path) -> PathBuf {
    if path.starts_with("~") {
        if let Some(home) = dirs::home_dir() {
            return home.join(path.strip_prefix("~").unwrap());
        }
    }
    path.to_path_buf()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config() {
        let config = Config::default();
        assert_eq!(config.server.port, 19420);
        assert_eq!(config.server.host, "127.0.0.1");
        assert!(config.server.api_key.is_none());
        assert!(!config.ai.enabled);
    }

    #[test]
    fn test_parse_config() {
        let toml = r#"
[server]
port = 9000
host = "0.0.0.0"

[[watch]]
path = "~/.claude/projects"
parser = "claude_code"

[ai]
enabled = true
provider = "anthropic"

[ai.features]
title_generation = true
skills_discovery = false
"#;

        let config: Config = toml::from_str(toml).unwrap();
        assert_eq!(config.server.port, 9000);
        assert_eq!(config.server.host, "0.0.0.0");
        assert_eq!(config.watch.len(), 1);
        assert!(config.ai.enabled);
        assert!(config.ai.features.title_generation);
        assert!(!config.ai.features.skills_discovery);
    }
}
