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

    /// Watch paths for session files
    #[serde(default, alias = "projects")]
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

    /// Enable mDNS/Bonjour service discovery on the local network.
    /// Auto-disabled when host is 127.0.0.1 (localhost-only).
    #[serde(default = "default_true")]
    pub mdns_enabled: bool,

    /// Custom instance name for mDNS announcement.
    /// If not set, uses "Yocore-{hostname}-{short_uuid}".
    #[serde(default)]
    pub instance_name: Option<String>,
}

fn default_port() -> u16 {
    19420 // Uncommon port to avoid conflicts
}

fn default_host() -> String {
    "127.0.0.1".to_string() // Localhost only - secure by default
}

impl ServerConfig {
    /// Check if mDNS should be active based on host binding and config.
    /// Returns false for localhost-only bindings since there's nothing to discover.
    pub fn should_enable_mdns(&self) -> bool {
        if self.host == "127.0.0.1" || self.host == "localhost" {
            return false;
        }
        self.mdns_enabled
    }
}

impl Default for ServerConfig {
    fn default() -> Self {
        ServerConfig {
            port: default_port(),
            host: default_host(),
            api_key: None,
            mdns_enabled: true,
            instance_name: None,
        }
    }
}

/// Watch configuration — defines a directory to watch for session files
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WatchConfig {
    /// Path to watch for session files (parent directory containing project subdirs)
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
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct AiConfig {
    /// Whether AI features are enabled
    #[serde(default)]
    pub enabled: bool,

    /// AI provider ("claude_code" for CLI mode)
    #[serde(default)]
    pub provider: Option<String>,

    /// AI feature toggles
    #[serde(default)]
    pub features: AiFeatures,
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

    /// Memory ranking configuration
    #[serde(default)]
    pub ranking: RankingConfig,

    /// Duplicate memory cleanup configuration
    #[serde(default)]
    pub duplicate_cleanup: DuplicateCleanupConfig,

    /// Embedding refresh configuration
    #[serde(default)]
    pub embedding_refresh: EmbeddingRefreshConfig,

    /// Duplicate skill cleanup configuration
    #[serde(default)]
    pub skill_cleanup: SkillCleanupConfig,
}

/// Memory ranking configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RankingConfig {
    /// Whether automatic memory ranking is enabled
    #[serde(default = "default_true")]
    pub enabled: bool,

    /// Interval in hours between periodic ranking sweeps
    #[serde(default = "default_ranking_interval")]
    pub interval_hours: u32,

    /// Number of memories to process per batch
    #[serde(default = "default_batch_size")]
    pub batch_size: usize,
}

fn default_ranking_interval() -> u32 {
    6 // Every 6 hours
}

fn default_batch_size() -> usize {
    500
}

impl Default for RankingConfig {
    fn default() -> Self {
        RankingConfig {
            enabled: true,
            interval_hours: default_ranking_interval(),
            batch_size: default_batch_size(),
        }
    }
}

/// Duplicate memory cleanup configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DuplicateCleanupConfig {
    /// Whether automatic duplicate cleanup is enabled
    #[serde(default)]
    pub enabled: bool,

    /// Interval in hours between cleanup sweeps
    #[serde(default = "default_cleanup_interval")]
    pub interval_hours: u32,

    /// Similarity threshold for detecting duplicates (stricter than extraction's 0.65)
    #[serde(default = "default_similarity_threshold")]
    pub similarity_threshold: f64,

    /// Number of memories to process per batch
    #[serde(default = "default_batch_size")]
    pub batch_size: usize,
}

fn default_cleanup_interval() -> u32 {
    24 // Every 24 hours
}

fn default_similarity_threshold() -> f64 {
    0.75 // Stricter than extraction (0.65) to minimize false positives
}

impl Default for DuplicateCleanupConfig {
    fn default() -> Self {
        DuplicateCleanupConfig {
            enabled: false, // Opt-in
            interval_hours: default_cleanup_interval(),
            similarity_threshold: default_similarity_threshold(),
            batch_size: default_batch_size(),
        }
    }
}

/// Embedding refresh configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EmbeddingRefreshConfig {
    /// Whether automatic embedding refresh is enabled
    #[serde(default = "default_true")]
    pub enabled: bool,

    /// Interval in hours between refresh sweeps
    #[serde(default = "default_refresh_interval")]
    pub interval_hours: u32,

    /// Number of memories to embed per batch (lower than ranking — embeddings are CPU-heavy)
    #[serde(default = "default_embed_batch_size")]
    pub batch_size: usize,
}

fn default_refresh_interval() -> u32 {
    12 // Every 12 hours
}

fn default_embed_batch_size() -> usize {
    100 // Smaller batch — embeddings are CPU-intensive
}

impl Default for EmbeddingRefreshConfig {
    fn default() -> Self {
        EmbeddingRefreshConfig {
            enabled: true,
            interval_hours: default_refresh_interval(),
            batch_size: default_embed_batch_size(),
        }
    }
}

/// Duplicate skill cleanup configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkillCleanupConfig {
    /// Whether automatic skill cleanup is enabled
    #[serde(default)]
    pub enabled: bool,

    /// Interval in hours between cleanup sweeps
    #[serde(default = "default_cleanup_interval")]
    pub interval_hours: u32,

    /// Similarity threshold for detecting duplicate skills (stricter than extraction's 0.70)
    #[serde(default = "default_skill_similarity_threshold")]
    pub similarity_threshold: f64,

    /// Number of skills to process per batch
    #[serde(default = "default_batch_size")]
    pub batch_size: usize,
}

fn default_skill_similarity_threshold() -> f64 {
    0.80 // Stricter than extraction (0.70) to minimize false positives
}

impl Default for SkillCleanupConfig {
    fn default() -> Self {
        SkillCleanupConfig {
            enabled: false, // Opt-in
            interval_hours: default_cleanup_interval(),
            similarity_threshold: default_skill_similarity_threshold(),
            batch_size: default_batch_size(),
        }
    }
}

impl Default for AiFeatures {
    fn default() -> Self {
        AiFeatures {
            title_generation: true,
            skills_discovery: true,
            memory_extraction: true,
            ranking: RankingConfig::default(),
            duplicate_cleanup: DuplicateCleanupConfig::default(),
            embedding_refresh: EmbeddingRefreshConfig::default(),
            skill_cleanup: SkillCleanupConfig::default(),
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

    /// Save configuration to a TOML file
    pub fn save_to_file<P: AsRef<Path>>(&self, path: P) -> Result<()> {
        let content = toml::to_string_pretty(self)
            .map_err(|e| CoreError::Config(format!("Failed to serialize config: {}", e)))?;
        std::fs::write(path.as_ref(), content)?;
        Ok(())
    }

    /// Check if config is read-only (via YOLOG_CONFIG_READONLY env var)
    pub fn is_readonly() -> bool {
        std::env::var("YOLOG_CONFIG_READONLY")
            .map(|v| v == "true" || v == "1")
            .unwrap_or(false)
    }

    /// Get list of active environment overrides
    pub fn active_env_overrides() -> Vec<String> {
        let mut overrides = Vec::new();
        if std::env::var("YOLOG_SERVER_HOST").is_ok() {
            overrides.push("YOLOG_SERVER_HOST".to_string());
        }
        if std::env::var("YOLOG_SERVER_PORT").is_ok() {
            overrides.push("YOLOG_SERVER_PORT".to_string());
        }
        if std::env::var("YOLOG_SERVER_API_KEY").is_ok() {
            overrides.push("YOLOG_SERVER_API_KEY".to_string());
        }
        if std::env::var("YOLOG_DATA_DIR").is_ok() {
            overrides.push("YOLOG_DATA_DIR".to_string());
        }
        if std::env::var("YOLOG_CONFIG_READONLY").is_ok() {
            overrides.push("YOLOG_CONFIG_READONLY".to_string());
        }
        overrides
    }

    /// Apply environment variable overrides (server options only)
    pub fn apply_env_overrides(&mut self) {
        if let Ok(host) = std::env::var("YOLOG_SERVER_HOST") {
            self.server.host = host;
        }
        if let Ok(port) = std::env::var("YOLOG_SERVER_PORT") {
            if let Ok(port) = port.parse() {
                self.server.port = port;
            }
        }
        if let Ok(key) = std::env::var("YOLOG_SERVER_API_KEY") {
            self.server.api_key = if key.is_empty() { None } else { Some(key) };
        }
        if let Ok(data_dir) = std::env::var("YOLOG_DATA_DIR") {
            self.data_dir = PathBuf::from(data_dir);
        }
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

# Friendly nickname for this instance (shown in mDNS discovery)
# instance_name = "My Mac mini"

# Directories to watch for session files
# Projects are auto-created when sessions are discovered.
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
# provider = "claude_code"

[ai.features]
title_generation = true
skills_discovery = true
memory_extraction = true

[ai.features.ranking]
enabled = true
interval_hours = 6
batch_size = 500

[ai.features.duplicate_cleanup]
# Retroactive duplicate memory detection and removal
enabled = false
interval_hours = 24
similarity_threshold = 0.75
batch_size = 500

[ai.features.embedding_refresh]
# Backfill embeddings for memories missing them
enabled = true
interval_hours = 12
batch_size = 100

[ai.features.skill_cleanup]
# Retroactive duplicate skill detection and removal
enabled = false
interval_hours = 24
similarity_threshold = 0.80
batch_size = 500
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
pub fn expand_path(path: &Path) -> PathBuf {
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
provider = "claude_code"

[ai.features]
title_generation = true
skills_discovery = false
"#;

        let config: Config = toml::from_str(toml).unwrap();
        assert_eq!(config.server.port, 9000);
        assert_eq!(config.server.host, "0.0.0.0");
        assert_eq!(config.watch.len(), 1);
        assert_eq!(config.watch[0].parser, "claude_code");
        assert!(config.ai.enabled);
        assert!(config.ai.features.title_generation);
        assert!(!config.ai.features.skills_discovery);
    }

    #[test]
    fn test_backward_compat_projects_alias() {
        let toml = r#"
[[projects]]
path = "~/.claude/projects"
parser = "claude_code"
"#;

        let config: Config = toml::from_str(toml).unwrap();
        assert_eq!(config.watch.len(), 1);
        assert_eq!(config.watch[0].parser, "claude_code");
    }
}
