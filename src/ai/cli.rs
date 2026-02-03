//! CLI Detection and Invocation
//!
//! Detects installed AI CLI tools and invokes them for AI operations.

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
    // Future: Gemini, Copilot, etc.
}

impl CliProvider {
    /// Display name for the provider
    pub fn display_name(&self) -> &'static str {
        match self {
            CliProvider::ClaudeCode => "Claude Code",
        }
    }

    /// Command name to execute
    pub fn command_name(&self) -> &'static str {
        match self {
            CliProvider::ClaudeCode => "claude",
        }
    }

    /// Timeout for title generation
    pub fn title_timeout(&self) -> Duration {
        match self {
            CliProvider::ClaudeCode => Duration::from_secs(60),
        }
    }

    /// Timeout for memory/skill extraction
    pub fn extraction_timeout(&self) -> Duration {
        match self {
            CliProvider::ClaudeCode => Duration::from_secs(120),
        }
    }

    /// Build CLI arguments for prompt execution
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
            ],
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

/// Detect if Claude Code CLI is installed
pub async fn detect_claude_code() -> DetectedCli {
    let provider = CliProvider::ClaudeCode;

    // Common installation paths for Claude Code
    let common_paths = get_common_paths();

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
    if let Some(path) = find_in_path("claude").await {
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

/// Get common installation paths for Claude Code CLI
fn get_common_paths() -> Vec<PathBuf> {
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

/// Find a command in PATH
async fn find_in_path(command: &str) -> Option<PathBuf> {
    #[cfg(unix)]
    let which_cmd = "which";
    #[cfg(windows)]
    let which_cmd = "where";

    let output = Command::new(which_cmd)
        .arg(command)
        .output()
        .await
        .ok()?;

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
        Command::new(path)
            .arg("--version")
            .output()
            .await
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

    let result = timeout(timeout_duration, async {
        Command::new(path)
            .args(&args)
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

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_detect_claude_code() {
        let detected = detect_claude_code().await;
        // Just verify the detection doesn't panic
        println!("Claude Code detected: {:?}", detected);
    }
}
