//! Configuration management for Yolog Core
//!
//! Loads settings from TOML file at ~/.yolog/config.toml

use crate::error::{CoreError, Result};
use serde::{Deserialize, Serialize};
use std::net::SocketAddr;
use std::path::{Path, PathBuf};

/// Storage backend
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum Storage {
    /// SQLite database — full history, search, persistence (default)
    #[default]
    Db,
    /// In-memory only — no database, no persistence, data lost on restart
    Ephemeral,
}

impl Storage {
    pub fn is_db(&self) -> bool {
        matches!(self, Storage::Db)
    }

    pub fn is_ephemeral(&self) -> bool {
        matches!(self, Storage::Ephemeral)
    }
}

/// Ephemeral storage limits (only used when storage = "ephemeral")
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EphemeralConfig {
    /// Maximum sessions to keep in memory (LRU eviction)
    #[serde(default = "default_max_sessions")]
    pub max_sessions: usize,

    /// Maximum messages per session
    #[serde(default = "default_max_messages_per_session")]
    pub max_messages_per_session: usize,
}

fn default_max_sessions() -> usize {
    100
}

fn default_max_messages_per_session() -> usize {
    50
}

impl Default for EphemeralConfig {
    fn default() -> Self {
        EphemeralConfig {
            max_sessions: default_max_sessions(),
            max_messages_per_session: default_max_messages_per_session(),
        }
    }
}

/// AI feature identifier for feature gating
#[derive(Debug, Clone, Copy)]
pub enum AiFeature {
    TitleGeneration,
    MarkerDetection,
    MemoryExtraction,
    SkillsDiscovery,
}

/// Main configuration structure
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    /// Storage backend: "db" (default) or "ephemeral"
    #[serde(default)]
    pub storage: Storage,

    /// Server configuration
    #[serde(default)]
    pub server: ServerConfig,

    /// Watch paths for session files
    #[serde(default, alias = "projects")]
    pub watch: Vec<WatchConfig>,

    /// AI feature configuration
    #[serde(default)]
    pub ai: AiConfig,

    /// Background scheduler task configuration
    #[serde(default)]
    pub scheduler: SchedulerConfig,

    /// Ephemeral storage limits
    #[serde(default)]
    pub ephemeral: EphemeralConfig,

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
///
/// AI is active when `provider` is set and at least one feature toggle is true.
/// Features requiring persistence (marker_detection, memory_extraction, skills_discovery)
/// are automatically skipped when `storage = "ephemeral"`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AiConfig {
    /// AI provider ("claude_code" for CLI mode). Required for any AI feature.
    #[serde(default)]
    pub provider: Option<String>,

    /// Generate session titles (works with both storage modes)
    #[serde(default = "default_true")]
    pub title_generation: bool,

    /// Detect markers in sessions (requires storage = "db")
    #[serde(default = "default_true")]
    pub marker_detection: bool,

    /// Extract memories from sessions (requires storage = "db")
    #[serde(default = "default_true")]
    pub memory_extraction: bool,

    /// Discover skills from sessions (requires storage = "db")
    #[serde(default = "default_true")]
    pub skills_discovery: bool,

    // Legacy fields for backward compatibility — not serialized
    /// Deprecated: AI is now active when provider is set + any feature is on
    #[serde(default, skip_serializing)]
    enabled: Option<bool>,

    /// Deprecated: features are now flat fields in [ai]
    #[serde(default, skip_serializing)]
    features: Option<LegacyAiFeatures>,
}

/// Legacy [ai.features] section — only used for backward-compatible deserialization
#[derive(Debug, Clone, Deserialize)]
struct LegacyAiFeatures {
    #[serde(default = "default_true")]
    title_generation: bool,
    #[serde(default = "default_true")]
    skills_discovery: bool,
    #[serde(default = "default_true")]
    memory_extraction: bool,
}

/// Background scheduler configuration
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct SchedulerConfig {
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
///
/// Auto-activated when memory_extraction is enabled.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RankingConfig {
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
            interval_hours: default_ranking_interval(),
            batch_size: default_batch_size(),
        }
    }
}

/// Duplicate memory cleanup configuration
///
/// Auto-activated when memory_extraction is enabled.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DuplicateCleanupConfig {
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
            interval_hours: default_cleanup_interval(),
            similarity_threshold: default_similarity_threshold(),
            batch_size: default_batch_size(),
        }
    }
}

/// Embedding refresh configuration
///
/// Auto-activated when memory_extraction is enabled.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EmbeddingRefreshConfig {
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
            interval_hours: default_refresh_interval(),
            batch_size: default_embed_batch_size(),
        }
    }
}

/// Duplicate skill cleanup configuration
///
/// Auto-activated when skills_discovery is enabled.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkillCleanupConfig {
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
            interval_hours: default_cleanup_interval(),
            similarity_threshold: default_skill_similarity_threshold(),
            batch_size: default_batch_size(),
        }
    }
}

impl Default for AiConfig {
    fn default() -> Self {
        AiConfig {
            provider: None,
            title_generation: true,
            marker_detection: true,
            memory_extraction: true,
            skills_discovery: true,
            enabled: None,
            features: None,
        }
    }
}

impl AiConfig {
    /// Apply legacy config fields for backward compatibility.
    ///
    /// Handles old config format where features lived in [ai.features] and
    /// AI was gated by [ai] enabled = true/false.
    fn apply_legacy(&mut self) {
        // If old `enabled = false` is present, disable AI by clearing provider
        if let Some(false) = self.enabled.take() {
            self.provider = None;
        }

        // If old [ai.features] section is present, its values override the flat defaults
        if let Some(features) = self.features.take() {
            self.title_generation = features.title_generation;
            self.skills_discovery = features.skills_discovery;
            self.memory_extraction = features.memory_extraction;
        }
    }
}

impl Default for Config {
    fn default() -> Self {
        Config {
            storage: Storage::default(),
            server: ServerConfig::default(),
            watch: vec![],
            ai: AiConfig::default(),
            scheduler: SchedulerConfig::default(),
            ephemeral: EphemeralConfig::default(),
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
        let mut config: Config = toml::from_str(&content)?;
        config.ai.apply_legacy();

        Ok(config)
    }

    /// Check if a specific AI feature is active given current config.
    ///
    /// Returns false if provider is not set, or if the feature requires
    /// db storage but storage is ephemeral.
    pub fn is_feature_active(&self, feature: AiFeature) -> bool {
        if self.ai.provider.is_none() {
            return false;
        }
        match feature {
            AiFeature::TitleGeneration => self.ai.title_generation,
            AiFeature::MarkerDetection => self.ai.marker_detection && self.storage.is_db(),
            AiFeature::MemoryExtraction => self.ai.memory_extraction && self.storage.is_db(),
            AiFeature::SkillsDiscovery => self.ai.skills_discovery && self.storage.is_db(),
        }
    }

    /// Check if any AI feature is active
    pub fn is_ai_active(&self) -> bool {
        self.ai.provider.is_some()
            && (self.ai.title_generation
                || self.ai.marker_detection
                || self.ai.memory_extraction
                || self.ai.skills_discovery)
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

# Storage backend:
#   "db"        — SQLite database. Full history, search, persistence. (default)
#   "ephemeral" — In-memory only. No database, no persistence. Data lost on restart.
storage = "db"

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

# Ephemeral storage limits (only used when storage = "ephemeral")
# [ephemeral]
# max_sessions = 100
# max_messages_per_session = 50

# AI features — each toggle is independent, some require storage = "db"
# AI is active when provider is set and at least one feature is enabled.
[ai]
# provider = "claude_code"     # Claude Code CLI
# provider = "openclaw"        # OpenClaw CLI (requires gateway)
title_generation = true
marker_detection = true
memory_extraction = true
skills_discovery = true

# Background scheduler tasks
# Auto-activated by their parent AI features — no individual enabled flags.
# memory_extraction activates: ranking, duplicate_cleanup, embedding_refresh
# skills_discovery activates: skill_cleanup

[scheduler.ranking]
interval_hours = 6
batch_size = 500

[scheduler.duplicate_cleanup]
interval_hours = 24
similarity_threshold = 0.75
batch_size = 500

[scheduler.embedding_refresh]
interval_hours = 12
batch_size = 100

[scheduler.skill_cleanup]
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
        assert!(config.ai.provider.is_none());
        assert_eq!(config.storage, Storage::Db);
    }

    #[test]
    fn test_parse_new_config_format() {
        let toml = r#"
storage = "db"

[server]
port = 9000
host = "0.0.0.0"

[[watch]]
path = "~/.claude/projects"
parser = "claude_code"

[ai]
provider = "claude_code"
title_generation = true
skills_discovery = false
"#;

        let config: Config = toml::from_str(toml).unwrap();
        assert_eq!(config.server.port, 9000);
        assert_eq!(config.server.host, "0.0.0.0");
        assert_eq!(config.storage, Storage::Db);
        assert_eq!(config.watch.len(), 1);
        assert_eq!(config.watch[0].parser, "claude_code");
        assert_eq!(config.ai.provider.as_deref(), Some("claude_code"));
        assert!(config.ai.title_generation);
        assert!(!config.ai.skills_discovery);
    }

    #[test]
    fn test_parse_legacy_config_format() {
        let toml = r#"
[server]
port = 9000

[ai]
enabled = true
provider = "claude_code"

[ai.features]
title_generation = true
skills_discovery = false
memory_extraction = true
"#;

        let mut config: Config = toml::from_str(toml).unwrap();
        config.ai.apply_legacy();

        assert_eq!(config.ai.provider.as_deref(), Some("claude_code"));
        assert!(config.ai.title_generation);
        assert!(!config.ai.skills_discovery);
        assert!(config.ai.memory_extraction);
    }

    #[test]
    fn test_legacy_enabled_false_clears_provider() {
        let toml = r#"
[ai]
enabled = false
provider = "claude_code"
"#;

        let mut config: Config = toml::from_str(toml).unwrap();
        config.ai.apply_legacy();

        assert!(config.ai.provider.is_none());
    }

    #[test]
    fn test_parse_openclaw_provider() {
        let toml = r#"
[ai]
provider = "openclaw"
title_generation = true
"#;
        let config: Config = toml::from_str(toml).unwrap();
        assert_eq!(config.ai.provider.as_deref(), Some("openclaw"));
        assert!(config.is_ai_active());
        assert!(config.is_feature_active(AiFeature::TitleGeneration));
    }

    #[test]
    fn test_is_feature_active() {
        let mut config = Config::default();
        config.ai.provider = Some("claude_code".to_string());

        // All features active with db storage + provider
        assert!(config.is_feature_active(AiFeature::TitleGeneration));
        assert!(config.is_feature_active(AiFeature::MemoryExtraction));
        assert!(config.is_feature_active(AiFeature::SkillsDiscovery));
        assert!(config.is_feature_active(AiFeature::MarkerDetection));

        // Ephemeral: only title_generation works
        config.storage = Storage::Ephemeral;
        assert!(config.is_feature_active(AiFeature::TitleGeneration));
        assert!(!config.is_feature_active(AiFeature::MemoryExtraction));
        assert!(!config.is_feature_active(AiFeature::SkillsDiscovery));
        assert!(!config.is_feature_active(AiFeature::MarkerDetection));

        // No provider: nothing active
        config.ai.provider = None;
        assert!(!config.is_feature_active(AiFeature::TitleGeneration));
    }

    #[test]
    fn test_ephemeral_storage_mode() {
        let toml = r#"
storage = "ephemeral"

[ephemeral]
max_sessions = 50
max_messages_per_session = 2000
"#;

        let config: Config = toml::from_str(toml).unwrap();
        assert_eq!(config.storage, Storage::Ephemeral);
        assert!(config.storage.is_ephemeral());
        assert!(!config.storage.is_db());
        assert_eq!(config.ephemeral.max_sessions, 50);
        assert_eq!(config.ephemeral.max_messages_per_session, 2000);
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

    #[test]
    fn test_scheduler_ignores_legacy_enabled_field() {
        let toml = r#"
[scheduler.ranking]
enabled = true
interval_hours = 12
batch_size = 1000
"#;

        // Should parse without error even though `enabled` is no longer a field
        let config: Config = toml::from_str(toml).unwrap();
        assert_eq!(config.scheduler.ranking.interval_hours, 12);
        assert_eq!(config.scheduler.ranking.batch_size, 1000);
    }
}
