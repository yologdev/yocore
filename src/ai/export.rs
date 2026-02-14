//! AI Export Generation
//!
//! Processes session content through Claude Code CLI to produce
//! structured exports (Dev Notes, Blog Posts). Supports single-request
//! and chunked map-reduce flows for large sessions.

use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

use super::cli::{detect_claude_code, run_cli, DetectedCli};

/// Maximum input length to send to CLI
pub const MAX_INPUT_LENGTH: usize = 100_000;

/// Export format options
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ExportFormat {
    Raw,
    TechnicalSummary,
    HighlightReel,
}

impl ExportFormat {
    /// Parse from string (matches kebab-case serde format)
    pub fn parse_format(s: &str) -> Option<Self> {
        match s {
            "raw" => Some(Self::Raw),
            "technical-summary" => Some(Self::TechnicalSummary),
            "highlight-reel" => Some(Self::HighlightReel),
            _ => None,
        }
    }

    /// Timeout for AI generation (3 min — export prompts produce long structured output)
    pub fn timeout(&self) -> Duration {
        match self {
            Self::Raw => Duration::from_secs(0),
            _ => Duration::from_secs(180),
        }
    }
}

/// Request to generate AI export
#[derive(Debug, Clone, Deserialize)]
pub struct GenerateExportRequest {
    pub session_id: String,
    pub format: String,
    pub raw_content: String,
}

/// Result from AI export generation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExportResult {
    pub content: String,
    pub format: String,
    pub provider: String,
    pub generation_time_ms: u64,
}

/// Request to process a single chunk
#[derive(Debug, Clone, Deserialize)]
pub struct ChunkRequest {
    pub format: String,
    pub chunk_content: String,
    pub chunk_index: usize,
    pub total_chunks: usize,
    pub is_first: bool,
    pub is_last: bool,
    pub target_output_chars: usize,
}

/// Result from processing a single chunk
#[derive(Debug, Clone, Serialize)]
pub struct ChunkResult {
    pub content: String,
    pub chunk_index: usize,
    pub provider: String,
    pub generation_time_ms: u64,
}

/// Request to merge partial chunk results
#[derive(Debug, Clone, Deserialize)]
pub struct MergeRequest {
    pub format: String,
    pub partial_results: Vec<String>,
}

/// Provider capabilities for export decisions
#[derive(Debug, Serialize)]
pub struct ProviderCapabilities {
    pub max_content_size: usize,
    pub timeout_secs: u64,
    pub supports_chunking: bool,
}

// ============================================================================
// Prompts (ported from release repo)
// ============================================================================

fn get_format_prompt(format: ExportFormat, content: &str) -> String {
    match format {
        ExportFormat::Raw => content.to_string(),

        ExportFormat::TechnicalSummary => format!(
            "Create a technical summary of this AI coding session (target: 2-5 pages).\n\n\
Include these sections:\n\
1. **Overview** (2-3 sentences): What was built or accomplished\n\
2. **Key Decisions**: Architecture choices and trade-offs\n\
3. **Important Changes**: File edits, new code (include brief snippets)\n\
4. **Problems & Solutions**: Issues and how they were resolved\n\
5. **Tools Used**: Summary of significant tool calls\n\
6. **Final State**: What was accomplished, any remaining work\n\n\
Write in professional technical documentation style. Use markdown formatting.\n\n\
IMPORTANT: Never include literal triple backticks as examples within code blocks. \
If showing code block syntax, describe it in words instead.\n\n\
Session content:\n{}",
            content
        ),

        ExportFormat::HighlightReel => format!(
            "Create a highlight reel of this AI coding session for a dev blog post.\n\n\
CRITICAL: Output ONLY the content. No meta-commentary like \"Here's what I found\" or \
\"Why this story works\". Just the actual blog post content.\n\n\
Required structure:\n\
1. **Title**: Catchy headline summarizing the journey (e.g., \"Building X: From Y to Z\")\n\
2. **TL;DR**: 1-2 sentences summarizing what was built and the key insight\n\
3. **The Challenge**: What problem was being solved\n\
4. **The Journey**: 2-4 sections with markdown headers telling the story\n\
   - Include specific numbers/metrics when available\n\
   - Include 2-3 relevant code snippets\n\
   - Show the progression and pivotal moments\n\
5. **Key Takeaway**: The main lesson learned (1-2 paragraphs)\n\
6. **Impact section** at the end with: Tech stack, files changed, what was accomplished\n\n\
Style:\n\
- First person, conversational but professional\n\
- Concrete details over vague descriptions\n\
- Show don't tell - use actual code and numbers\n\
- Target length: 1-2 pages\n\
- Never include literal triple backticks as examples within code blocks\n\n\
Session content:\n{}",
            content
        ),
    }
}

fn get_chunk_prompt(
    format: ExportFormat,
    content: &str,
    chunk_index: usize,
    total_chunks: usize,
    is_first: bool,
    is_last: bool,
    target_output_chars: usize,
) -> String {
    let position_context = if is_first {
        "This is the START of the session - capture initial context and setup."
    } else if is_last {
        "This is the END of the session - capture final state and outcomes."
    } else {
        "This is a MIDDLE segment - focus on key events and changes."
    };

    let size_constraint = format!(
        "IMPORTANT: Keep your output under {} characters (~{} words) to fit merge limits.",
        target_output_chars,
        target_output_chars / 5
    );

    match format {
        ExportFormat::Raw => content.to_string(),

        ExportFormat::TechnicalSummary => format!(
            "Analyze this PARTIAL segment (chunk {}/{}) of an AI coding session.\n\n\
{}\n\n\
{}\n\n\
Extract:\n\
1. Key decisions made in this segment\n\
2. Important code changes (include brief snippets, max 10 lines each)\n\
3. Problems encountered and solutions found\n\
4. Significant tool usage\n\n\
Output as structured markdown sections. Be concise - focus on the most important points.\n\n\
Segment content:\n{}",
            chunk_index + 1,
            total_chunks,
            position_context,
            size_constraint,
            content
        ),

        ExportFormat::HighlightReel => format!(
            "Extract highlights from this PARTIAL segment (chunk {}/{}) of an AI coding session.\n\n\
{}\n\n\
{}\n\n\
For each highlight include:\n\
- **What**: The specific event (1-2 sentences)\n\
- **Why**: The impact (1 sentence)\n\
- **Code**: Only if essential (max 10 lines)\n\n\
Focus on: breakthroughs, problems solved, clever solutions, key decisions.\n\
Output as concise bullet points.\n\n\
Segment content:\n{}",
            chunk_index + 1,
            total_chunks,
            position_context,
            size_constraint,
            content
        ),
    }
}

fn get_merge_prompt(format: ExportFormat, partial_results: &[String]) -> String {
    let combined = partial_results.join("\n\n---\n\n");

    match format {
        ExportFormat::Raw => combined,

        ExportFormat::TechnicalSummary => format!(
            "Consolidate these {} partial analyses into a cohesive technical summary (2-5 pages).\n\n\
Create these sections:\n\
1. **Overview** (2-3 sentences): What was built or accomplished\n\
2. **Key Decisions**: Architecture choices (deduplicate across segments)\n\
3. **Important Changes**: File edits, new code (include best snippets)\n\
4. **Problems & Solutions**: Issues and resolutions\n\
5. **Final State**: What was accomplished\n\n\
Write in professional technical documentation style.\n\
Never include literal triple backticks as examples within code blocks.\n\n\
Partial analyses:\n{}",
            partial_results.len(),
            combined
        ),

        ExportFormat::HighlightReel => format!(
            "Synthesize these {} highlight extractions into a dev blog post.\n\n\
CRITICAL: Output ONLY the blog post content. No meta-commentary like \"Here's what I found\" \
or \"Why this story works\". Just the actual content.\n\n\
Required structure:\n\
1. **Title**: Catchy headline (e.g., \"Building X: From Y to Z\")\n\
2. **TL;DR**: 1-2 sentences summarizing what was built and the key insight\n\
3. **The Challenge**: What problem was being solved\n\
4. **The Journey**: 2-4 sections with markdown headers telling the story\n\
   - Use the best highlights to create a narrative arc\n\
   - Include 2-3 code snippets from the extractions\n\
   - Include specific numbers/metrics\n\
5. **Key Takeaway**: The main lesson learned (1-2 paragraphs)\n\
6. **Impact**: Tech stack, what was accomplished\n\n\
Style:\n\
- First person, conversational but professional\n\
- Concrete details over vague descriptions\n\
- Show don't tell - use actual code and numbers from the highlights\n\
- Target length: 1-2 pages\n\
- Never include literal triple backticks as examples within code blocks\n\n\
Extracted highlights:\n{}",
            partial_results.len(),
            combined
        ),
    }
}

/// Truncate content to fit within CLI limits (UTF-8 safe)
fn truncate_content(content: &str, max_length: usize) -> String {
    if content.len() <= max_length {
        return content.to_string();
    }

    // Find a valid UTF-8 boundary at or before max_length
    let safe_end = content
        .char_indices()
        .take_while(|(i, _)| *i <= max_length)
        .last()
        .map(|(i, _)| i)
        .unwrap_or(0);
    let truncated = &content[..safe_end];

    // Try to truncate at a message boundary (### User / ### Assistant)
    if let Some(pos) = truncated.rfind("\n### ") {
        return format!(
            "{}\n\n[... content truncated for length ...]",
            &content[..pos]
        );
    }

    // Fallback: truncate at last newline
    if let Some(pos) = truncated.rfind('\n') {
        return format!(
            "{}\n\n[... content truncated for length ...]",
            &content[..pos]
        );
    }

    format!("{}...", truncated)
}

// ============================================================================
// Generation functions
// ============================================================================

/// Generate export content using AI CLI
pub async fn generate_export(
    content: &str,
    format: ExportFormat,
    cli: &DetectedCli,
) -> Result<ExportResult, String> {
    if format == ExportFormat::Raw {
        return Ok(ExportResult {
            content: content.to_string(),
            format: "raw".to_string(),
            provider: "none".to_string(),
            generation_time_ms: 0,
        });
    }

    let truncated = truncate_content(content, MAX_INPUT_LENGTH);
    let prompt = get_format_prompt(format, &truncated);
    let timeout = format.timeout();

    let start = Instant::now();
    let result = run_cli(cli, &prompt, timeout).await?;
    let generation_time_ms = start.elapsed().as_millis() as u64;

    if result.is_empty() {
        return Err("AI returned empty response. Please try again.".to_string());
    }

    let format_str = serde_json::to_value(format)
        .ok()
        .and_then(|v| v.as_str().map(|s| s.to_string()))
        .unwrap_or_else(|| "unknown".to_string());

    Ok(ExportResult {
        content: result,
        format: format_str,
        provider: cli.provider.display_name().to_string(),
        generation_time_ms,
    })
}

/// Process a single chunk of content
pub async fn process_chunk(
    request: &ChunkRequest,
    cli: &DetectedCli,
) -> Result<ChunkResult, String> {
    let format = ExportFormat::parse_format(&request.format)
        .ok_or_else(|| format!("Unknown format: {}", request.format))?;

    if format == ExportFormat::Raw {
        return Ok(ChunkResult {
            content: request.chunk_content.clone(),
            chunk_index: request.chunk_index,
            provider: "none".to_string(),
            generation_time_ms: 0,
        });
    }

    let prompt = get_chunk_prompt(
        format,
        &request.chunk_content,
        request.chunk_index,
        request.total_chunks,
        request.is_first,
        request.is_last,
        request.target_output_chars,
    );

    let timeout = Duration::from_secs(180);
    let start = Instant::now();
    let result = run_cli(cli, &prompt, timeout).await.map_err(|e| {
        format!(
            "Chunk {}/{} failed: {}",
            request.chunk_index + 1,
            request.total_chunks,
            e
        )
    })?;
    let generation_time_ms = start.elapsed().as_millis() as u64;

    if result.is_empty() {
        return Err(format!(
            "Chunk {}/{} returned empty",
            request.chunk_index + 1,
            request.total_chunks
        ));
    }

    Ok(ChunkResult {
        content: result,
        chunk_index: request.chunk_index,
        provider: cli.provider.display_name().to_string(),
        generation_time_ms,
    })
}

/// Merge partial results from chunks
pub async fn merge_chunks(
    request: &MergeRequest,
    cli: &DetectedCli,
) -> Result<ExportResult, String> {
    let format = ExportFormat::parse_format(&request.format)
        .ok_or_else(|| format!("Unknown format: {}", request.format))?;

    if format == ExportFormat::Raw {
        return Ok(ExportResult {
            content: request.partial_results.join("\n\n"),
            format: request.format.clone(),
            provider: "none".to_string(),
            generation_time_ms: 0,
        });
    }

    let prompt = get_merge_prompt(format, &request.partial_results);
    let timeout = Duration::from_secs(180);

    let start = Instant::now();
    let result = run_cli(cli, &prompt, timeout).await?;
    let generation_time_ms = start.elapsed().as_millis() as u64;

    if result.is_empty() {
        return Err("Merge returned empty response".to_string());
    }

    Ok(ExportResult {
        content: result,
        format: request.format.clone(),
        provider: cli.provider.display_name().to_string(),
        generation_time_ms,
    })
}

/// Detect CLI and return capabilities
pub async fn get_capabilities() -> ProviderCapabilities {
    ProviderCapabilities {
        max_content_size: MAX_INPUT_LENGTH,
        timeout_secs: 180,
        supports_chunking: true,
    }
}

/// Detect CLI availability (convenience wrapper)
pub async fn ensure_cli() -> Result<DetectedCli, String> {
    let cli = detect_claude_code().await;
    if !cli.installed {
        return Err("Claude Code CLI not installed. Please install it first.".to_string());
    }
    Ok(cli)
}
