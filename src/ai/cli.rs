//! CLI Detection and Invocation
//!
//! Detects installed AI CLI tools and invokes them for AI operations.
//! Provider-specific logic is encapsulated in `CliProvider` methods.
//! Adding a new provider requires only adding an enum variant and match arms here.

use std::path::PathBuf;
use std::process::Stdio;
use std::time::Duration;
use tokio::process::Command;
use tokio::time::timeout;

/// Supported AI CLI providers
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CliProvider {
    ClaudeCode,
    #[serde(rename = "openclaw")]
    OpenClaw,
}

impl CliProvider {
    /// Parse provider from config string value
    pub fn from_config_str(s: &str) -> Option<Self> {
        match s {
            "claude_code" => Some(CliProvider::ClaudeCode),
            "openclaw" => Some(CliProvider::OpenClaw),
            _ => None,
        }
    }

    /// Display name for the provider
    pub fn display_name(&self) -> &'static str {
        match self {
            CliProvider::ClaudeCode => "Claude Code",
            CliProvider::OpenClaw => "OpenClaw",
        }
    }

    /// Command name to execute
    pub fn command_name(&self) -> &'static str {
        match self {
            CliProvider::ClaudeCode => "claude",
            CliProvider::OpenClaw => "openclaw",
        }
    }

    /// Timeout for title generation
    pub fn title_timeout(&self) -> Duration {
        match self {
            CliProvider::ClaudeCode => Duration::from_secs(60),
            CliProvider::OpenClaw => Duration::from_secs(90),
        }
    }

    /// Timeout for memory/skill extraction
    pub fn extraction_timeout(&self) -> Duration {
        match self {
            CliProvider::ClaudeCode => Duration::from_secs(120),
            CliProvider::OpenClaw => Duration::from_secs(180),
        }
    }

    /// Build CLI arguments for text output
    pub fn build_args(&self, prompt: &str) -> Vec<String> {
        match self {
            CliProvider::ClaudeCode => vec![
                "-p".to_string(),
                prompt.to_string(),
                "--output-format".to_string(),
                "text".to_string(),
                "--model".to_string(),
                "sonnet".to_string(),
                // Prevent macOS permission dialogs
                "--strict-mcp-config".to_string(),
                "--disable-slash-commands".to_string(),
                // Print mode: don't create session files
                "--print".to_string(),
            ],
            CliProvider::OpenClaw => vec![
                "agent".to_string(),
                "--message".to_string(),
                prompt.to_string(),
                "--thinking".to_string(),
                "high".to_string(),
            ],
        }
    }

    /// Build CLI arguments for structured (JSON) output.
    /// Used by marker detection which needs parseable responses.
    /// Providers without a dedicated JSON output mode return the same as build_args.
    pub fn build_json_args(&self, prompt: &str) -> Vec<String> {
        match self {
            CliProvider::ClaudeCode => vec![
                "-p".to_string(),
                prompt.to_string(),
                "--output-format".to_string(),
                "json".to_string(),
                "--model".to_string(),
                "sonnet".to_string(),
                "--strict-mcp-config".to_string(),
                "--mcp-config".to_string(),
                r#"{"mcpServers":{}}"#.to_string(),
                "--disable-slash-commands".to_string(),
                "--print".to_string(),
            ],
            // OpenClaw has no JSON output mode; prompt asks for JSON directly
            CliProvider::OpenClaw => self.build_args(prompt),
        }
    }

    /// Whether CLI output is wrapped in a JSON envelope that needs unwrapping.
    /// Claude Code wraps results in `{"type":"result","result":"..."}` when using JSON output format.
    pub fn has_json_wrapper(&self) -> bool {
        match self {
            CliProvider::ClaudeCode => true,
            CliProvider::OpenClaw => false,
        }
    }

    /// Common installation paths for this provider's CLI binary
    pub fn common_paths(&self) -> Vec<PathBuf> {
        match self {
            CliProvider::ClaudeCode => get_claude_common_paths(),
            CliProvider::OpenClaw => get_openclaw_common_paths(),
        }
    }
}

/// Detected CLI information
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct DetectedCli {
    pub provider: CliProvider,
    pub installed: bool,
    pub path: Option<PathBuf>,
    pub version: Option<String>,
}

/// Detect if a CLI provider is installed
pub async fn detect_provider(provider: CliProvider) -> DetectedCli {
    let common_paths = provider.common_paths();
    let command_name = provider.command_name();

    // First, check common paths directly
    for path in &common_paths {
        if path.exists() {
            // Verify it's executable by checking version
            if let Some(version) = check_cli_version(path).await {
                return DetectedCli {
                    provider,
                    installed: true,
                    path: Some(path.clone()),
                    version: Some(version),
                };
            }
        }
    }

    // Fall back to which/where command
    if let Some(path) = find_in_path(command_name).await {
        if let Some(version) = check_cli_version(&path).await {
            return DetectedCli {
                provider,
                installed: true,
                path: Some(path),
                version: Some(version),
            };
        }
    }

    DetectedCli {
        provider,
        installed: false,
        path: None,
        version: None,
    }
}

/// Legacy wrapper — prefer detect_provider(CliProvider::ClaudeCode)
pub async fn detect_claude_code() -> DetectedCli {
    detect_provider(CliProvider::ClaudeCode).await
}

/// Common installation paths for Claude Code CLI
fn get_claude_common_paths() -> Vec<PathBuf> {
    let mut paths = Vec::new();

    if let Some(home) = dirs::home_dir() {
        // npm global installs
        paths.push(home.join(".npm-global/bin/claude"));
        paths.push(home.join(".nvm/versions/node").join("*").join("bin/claude"));

        // Direct installs
        paths.push(home.join(".claude/bin/claude"));
        paths.push(home.join(".local/bin/claude"));
    }

    // System paths
    paths.push(PathBuf::from("/usr/local/bin/claude"));
    paths.push(PathBuf::from("/opt/homebrew/bin/claude"));

    #[cfg(target_os = "windows")]
    {
        if let Some(appdata) = dirs::data_local_dir() {
            paths.push(appdata.join("Programs/claude/claude.exe"));
        }
        paths.push(PathBuf::from("C:/Program Files/claude/claude.exe"));
    }

    paths
}

/// Common installation paths for OpenClaw CLI
fn get_openclaw_common_paths() -> Vec<PathBuf> {
    let mut paths = Vec::new();

    if let Some(home) = dirs::home_dir() {
        // npm global installs
        paths.push(home.join(".npm-global/bin/openclaw"));
        paths.push(
            home.join(".nvm/versions/node")
                .join("*")
                .join("bin/openclaw"),
        );
        paths.push(home.join(".local/bin/openclaw"));
    }

    // System paths
    paths.push(PathBuf::from("/usr/local/bin/openclaw"));
    paths.push(PathBuf::from("/opt/homebrew/bin/openclaw"));

    #[cfg(target_os = "windows")]
    {
        if let Some(appdata) = dirs::data_local_dir() {
            paths.push(appdata.join("Programs/openclaw/openclaw.exe"));
        }
    }

    paths
}

/// Find a command in PATH
async fn find_in_path(command: &str) -> Option<PathBuf> {
    #[cfg(unix)]
    let which_cmd = "which";
    #[cfg(windows)]
    let which_cmd = "where";

    let output = Command::new(which_cmd).arg(command).output().await.ok()?;

    if output.status.success() {
        let path_str = String::from_utf8_lossy(&output.stdout);
        let path = path_str.lines().next()?.trim();
        Some(PathBuf::from(path))
    } else {
        None
    }
}

/// Check CLI version
async fn check_cli_version(path: &PathBuf) -> Option<String> {
    let output = timeout(Duration::from_secs(5), async {
        Command::new(path).arg("--version").output().await
    })
    .await
    .ok()?
    .ok()?;

    if output.status.success() {
        let version = String::from_utf8_lossy(&output.stdout);
        Some(version.trim().to_string())
    } else {
        None
    }
}

/// Synchronously detect available CLI for a given provider.
/// Returns None if the CLI is not installed.
pub fn detect_cli_sync(provider: CliProvider) -> Option<DetectedCli> {
    let rt = tokio::runtime::Handle::try_current()
        .map(|h| {
            std::thread::scope(|s| {
                s.spawn(|| {
                    h.block_on(async {
                        let detected = detect_provider(provider).await;
                        if detected.installed {
                            Some(detected)
                        } else {
                            None
                        }
                    })
                })
                .join()
                .unwrap()
            })
        })
        .unwrap_or_else(|_| {
            let rt = tokio::runtime::Runtime::new().ok()?;
            rt.block_on(async {
                let detected = detect_provider(provider).await;
                if detected.installed {
                    Some(detected)
                } else {
                    None
                }
            })
        });

    rt
}

/// Legacy wrapper — prefer detect_cli_sync(provider)
pub fn detect_cli() -> Option<DetectedCli> {
    detect_cli_sync(CliProvider::ClaudeCode)
}

/// Run CLI with a prompt and return the output
pub async fn run_cli(
    cli: &DetectedCli,
    prompt: &str,
    timeout_duration: Duration,
) -> Result<String, String> {
    let path = cli.path.as_ref().ok_or("CLI path not available")?;

    let args = cli.provider.build_args(prompt);

    tracing::debug!(
        "Running {} CLI: {} {:?}",
        cli.provider.display_name(),
        path.display(),
        &args[..2.min(args.len())]
    );

    // Run in temp directory to avoid creating session files in watched folders
    let temp_dir = std::env::temp_dir();

    let result = timeout(timeout_duration, async {
        Command::new(path)
            .args(&args)
            .current_dir(&temp_dir)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .output()
            .await
    })
    .await;

    match result {
        Ok(Ok(output)) => {
            if output.status.success() {
                let stdout = String::from_utf8_lossy(&output.stdout);
                Ok(stdout.trim().to_string())
            } else {
                let stderr = String::from_utf8_lossy(&output.stderr);
                Err(format!("CLI failed: {}", stderr.trim()))
            }
        }
        Ok(Err(e)) => Err(format!("Failed to execute CLI: {}", e)),
        Err(_) => Err(format!(
            "CLI timed out after {} seconds",
            timeout_duration.as_secs()
        )),
    }
}

/// Call CLI with a prompt and return the raw response.
/// Used for marker detection which needs structured (JSON) output.
pub async fn call_cli_with_prompt(
    prompt: &str,
    cli: &DetectedCli,
    timeout_secs: u64,
) -> Result<String, String> {
    let path = cli.path.as_ref().ok_or("CLI path not available")?;
    let timeout_duration = Duration::from_secs(timeout_secs);

    let args = cli.provider.build_json_args(prompt);

    // Run in temp directory to avoid creating session files in watched folders
    let temp_dir = std::env::temp_dir();

    let result = timeout(timeout_duration, async {
        Command::new(path)
            .args(&args)
            .current_dir(&temp_dir)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .output()
            .await
    })
    .await;

    match result {
        Ok(Ok(output)) => {
            let stdout = String::from_utf8_lossy(&output.stdout);
            let response = stdout.trim();

            if !output.status.success() {
                return Err(format!("CLI failed: {}", response));
            }

            if response.is_empty() {
                return Err("CLI returned empty response".to_string());
            }

            // Unwrap provider-specific JSON wrapper if present
            if cli.provider.has_json_wrapper() {
                if let Ok(wrapper) = serde_json::from_str::<serde_json::Value>(response) {
                    if wrapper.get("type").and_then(|v| v.as_str()) == Some("result") {
                        if let Some(content) = wrapper.get("result").and_then(|v| v.as_str()) {
                            return Ok(content.to_string());
                        }
                    }
                }
            }

            Ok(response.to_string())
        }
        Ok(Err(e)) => Err(format!("Failed to execute CLI: {}", e)),
        Err(_) => Err(format!("CLI timed out after {} seconds", timeout_secs)),
    }
}

/// Parse JSON response from CLI (handles markdown code blocks)
pub fn parse_json_response<T: serde::de::DeserializeOwned>(response: &str) -> Result<T, String> {
    // Try direct parse first
    if let Ok(result) = serde_json::from_str(response) {
        return Ok(result);
    }

    // Extract from markdown code block
    let json_str = if response.contains("```") {
        let lines: Vec<&str> = response.lines().collect();
        let mut in_block = false;
        let mut json_lines = Vec::new();

        for line in lines {
            if line.starts_with("```json") || (line.starts_with("```") && !in_block) {
                in_block = true;
                continue;
            }
            if line.starts_with("```") && in_block {
                break;
            }
            if in_block {
                json_lines.push(line);
            }
        }
        json_lines.join("\n")
    } else {
        response.to_string()
    };

    serde_json::from_str(&json_str).map_err(|e| format!("Failed to parse JSON: {}", e))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_from_config_str() {
        assert_eq!(
            CliProvider::from_config_str("claude_code"),
            Some(CliProvider::ClaudeCode)
        );
        assert_eq!(
            CliProvider::from_config_str("openclaw"),
            Some(CliProvider::OpenClaw)
        );
        assert_eq!(CliProvider::from_config_str("unknown"), None);
        assert_eq!(CliProvider::from_config_str(""), None);
    }

    #[test]
    fn test_display_names() {
        assert_eq!(CliProvider::ClaudeCode.display_name(), "Claude Code");
        assert_eq!(CliProvider::OpenClaw.display_name(), "OpenClaw");
    }

    #[test]
    fn test_command_names() {
        assert_eq!(CliProvider::ClaudeCode.command_name(), "claude");
        assert_eq!(CliProvider::OpenClaw.command_name(), "openclaw");
    }

    #[test]
    fn test_openclaw_build_args() {
        let args = CliProvider::OpenClaw.build_args("test prompt");
        assert_eq!(
            args,
            vec!["agent", "--message", "test prompt", "--thinking", "high"]
        );
    }

    #[test]
    fn test_openclaw_build_json_args_same_as_text() {
        let text_args = CliProvider::OpenClaw.build_args("test");
        let json_args = CliProvider::OpenClaw.build_json_args("test");
        assert_eq!(text_args, json_args);
    }

    #[test]
    fn test_claude_build_json_args_different_from_text() {
        let text_args = CliProvider::ClaudeCode.build_args("test");
        let json_args = CliProvider::ClaudeCode.build_json_args("test");
        assert_ne!(text_args, json_args);
        assert!(json_args.contains(&"json".to_string()));
    }

    #[test]
    fn test_has_json_wrapper() {
        assert!(CliProvider::ClaudeCode.has_json_wrapper());
        assert!(!CliProvider::OpenClaw.has_json_wrapper());
    }

    #[test]
    fn test_openclaw_higher_timeouts() {
        assert!(CliProvider::OpenClaw.title_timeout() >= CliProvider::ClaudeCode.title_timeout());
        assert!(
            CliProvider::OpenClaw.extraction_timeout()
                >= CliProvider::ClaudeCode.extraction_timeout()
        );
    }

    #[tokio::test]
    async fn test_detect_provider_claude_code() {
        let detected = detect_provider(CliProvider::ClaudeCode).await;
        assert_eq!(detected.provider, CliProvider::ClaudeCode);
        println!("Claude Code detected: {:?}", detected);
    }

    #[tokio::test]
    async fn test_detect_provider_openclaw() {
        let detected = detect_provider(CliProvider::OpenClaw).await;
        assert_eq!(detected.provider, CliProvider::OpenClaw);
        println!("OpenClaw detected: {:?}", detected);
    }
}
