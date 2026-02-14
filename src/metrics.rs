//! Shared metrics types and stream-json parser for Claude CLI output.
//!
//! Extracts rich per-tool breakdowns, navigation efficiency, fmm usage
//! tracking, and outcome metrics from Claude's stream-json JSONL output.

use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::time::Duration;

/// Per-tool detail: count + associated args (files, patterns, commands).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ToolDetail {
    pub count: u32,
    /// File paths for Read/Edit/Write, patterns for Glob/Grep, commands for Bash.
    pub args: Vec<String>,
}

/// Navigation efficiency metrics.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct NavigationMetrics {
    /// Unique files read during the run.
    pub unique_files_read: u32,
    /// Unique files edited during the run.
    pub unique_files_edited: u32,
    /// Turn number of the first edit/write (0 if none).
    pub first_edit_turn: u32,
    /// Turns spent exploring (before first edit).
    pub exploration_turns: u32,
    /// Turns spent implementing (from first edit onward).
    pub implementation_turns: u32,
}

/// FMM-specific usage tracking.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct FmmUsage {
    /// Number of .fmm sidecar files read.
    pub sidecars_read: u32,
    /// Number of fmm MCP tool calls.
    pub mcp_tool_calls: u32,
    /// Names of fmm-specific tools called.
    pub fmm_tool_names: Vec<String>,
}

/// Accumulated metrics from a Claude CLI run.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct RunMetrics {
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cache_read_tokens: u64,
    pub cache_creation_tokens: u64,
    pub cost_usd: f64,
    pub turns: u32,
    pub duration_ms: u64,
    pub tool_calls: u32,
    pub tools_by_name: HashMap<String, u32>,
    pub files_accessed: Vec<String>,
    pub read_calls: u32,
    pub success: bool,
    pub error: Option<String>,

    /// Per-tool detail with args.
    pub tool_details: HashMap<String, ToolDetail>,
    /// Navigation efficiency.
    pub navigation: NavigationMetrics,
    /// FMM-specific usage tracking.
    pub fmm_usage: FmmUsage,
}

/// Parsed output from a Claude CLI stream-json invocation.
#[derive(Debug, Clone)]
pub struct ParsedOutput {
    pub metrics: RunMetrics,
    pub response_text: String,
}

/// Parse Claude CLI stream-json output into metrics and response text.
///
/// The `fallback_duration` is used when the result event doesn't include `duration_ms`.
pub fn parse_stream_json(output: &str, fallback_duration: Duration) -> Result<ParsedOutput> {
    let mut metrics = RunMetrics::default();
    let mut response_text = String::new();
    let mut final_result: Option<serde_json::Value> = None;

    // Track per-turn state for navigation efficiency
    let mut current_turn: u32 = 0;
    let mut first_edit_turn: u32 = 0;
    let mut files_read_set: HashSet<String> = HashSet::new();
    let mut files_edited_set: HashSet<String> = HashSet::new();

    for line in output.lines() {
        if line.trim().is_empty() {
            continue;
        }

        let data: serde_json::Value = match serde_json::from_str(line) {
            Ok(v) => v,
            Err(_) => continue,
        };

        match data.get("type").and_then(|v| v.as_str()) {
            Some("assistant") => {
                current_turn += 1;

                if let Some(message) = data.get("message") {
                    if let Some(content) = message.get("content").and_then(|c| c.as_array()) {
                        for item in content {
                            match item.get("type").and_then(|t| t.as_str()) {
                                Some("tool_use") => {
                                    process_tool_use(
                                        item,
                                        &mut metrics,
                                        current_turn,
                                        &mut first_edit_turn,
                                        &mut files_read_set,
                                        &mut files_edited_set,
                                    );
                                }
                                Some("text") => {
                                    if let Some(text) = item.get("text").and_then(|t| t.as_str()) {
                                        response_text = text.to_string();
                                    }
                                }
                                _ => {}
                            }
                        }
                    }
                }
            }
            Some("result") => {
                final_result = Some(data.clone());

                if let Some(usage) = data.get("usage") {
                    metrics.input_tokens = usage
                        .get("input_tokens")
                        .and_then(|v| v.as_u64())
                        .unwrap_or(0);
                    metrics.output_tokens = usage
                        .get("output_tokens")
                        .and_then(|v| v.as_u64())
                        .unwrap_or(0);
                    metrics.cache_read_tokens = usage
                        .get("cache_read_input_tokens")
                        .and_then(|v| v.as_u64())
                        .unwrap_or(0);
                    metrics.cache_creation_tokens = usage
                        .get("cache_creation_input_tokens")
                        .and_then(|v| v.as_u64())
                        .unwrap_or(0);
                }

                metrics.cost_usd = data
                    .get("total_cost_usd")
                    .and_then(|v| v.as_f64())
                    .unwrap_or(0.0);
                metrics.turns = data.get("num_turns").and_then(|v| v.as_u64()).unwrap_or(0) as u32;
                metrics.duration_ms = data
                    .get("duration_ms")
                    .and_then(|v| v.as_u64())
                    .unwrap_or(fallback_duration.as_millis() as u64);

                if let Some(result_text) = data.get("result").and_then(|r| r.as_str()) {
                    if response_text.is_empty() {
                        response_text = result_text.to_string();
                    }
                }
            }
            _ => {}
        }
    }

    // Finalize success/error
    metrics.success = final_result
        .as_ref()
        .and_then(|r| r.get("is_error"))
        .and_then(|e| e.as_bool())
        .map(|e| !e)
        .unwrap_or(false);

    metrics.error = if !metrics.success {
        final_result
            .as_ref()
            .and_then(|r| r.get("subtype"))
            .and_then(|s| s.as_str())
            .map(|s| s.to_string())
    } else {
        None
    };

    // Compute navigation efficiency
    metrics.navigation.unique_files_read = files_read_set.len() as u32;
    metrics.navigation.unique_files_edited = files_edited_set.len() as u32;
    metrics.navigation.first_edit_turn = first_edit_turn;
    if first_edit_turn > 0 {
        metrics.navigation.exploration_turns = first_edit_turn - 1;
        metrics.navigation.implementation_turns = current_turn.saturating_sub(first_edit_turn - 1);
    } else {
        metrics.navigation.exploration_turns = current_turn;
        metrics.navigation.implementation_turns = 0;
    }

    Ok(ParsedOutput {
        metrics,
        response_text,
    })
}

/// Process a single tool_use item from stream-json content.
fn process_tool_use(
    item: &serde_json::Value,
    metrics: &mut RunMetrics,
    current_turn: u32,
    first_edit_turn: &mut u32,
    files_read_set: &mut HashSet<String>,
    files_edited_set: &mut HashSet<String>,
) {
    metrics.tool_calls += 1;

    let Some(name) = item.get("name").and_then(|n| n.as_str()) else {
        return;
    };

    *metrics.tools_by_name.entry(name.to_string()).or_insert(0) += 1;

    let input = item.get("input");
    let detail = metrics.tool_details.entry(name.to_string()).or_default();
    detail.count += 1;

    match name {
        "Read" | "View" => {
            metrics.read_calls += 1;
            if let Some(input) = input {
                if let Some(path) = input
                    .get("file_path")
                    .or(input.get("path"))
                    .and_then(|p| p.as_str())
                {
                    metrics.files_accessed.push(path.to_string());
                    detail.args.push(path.to_string());
                    files_read_set.insert(path.to_string());

                    // Track fmm sidecar reads
                    if path.ends_with(".fmm") {
                        metrics.fmm_usage.sidecars_read += 1;
                    }
                }
            }
        }
        "Edit" => {
            if let Some(input) = input {
                if let Some(path) = input.get("file_path").and_then(|p| p.as_str()) {
                    detail.args.push(path.to_string());
                    files_edited_set.insert(path.to_string());
                }
            }
            if *first_edit_turn == 0 {
                *first_edit_turn = current_turn;
            }
        }
        "Write" => {
            if let Some(input) = input {
                if let Some(path) = input.get("file_path").and_then(|p| p.as_str()) {
                    detail.args.push(path.to_string());
                    files_edited_set.insert(path.to_string());
                }
            }
            if *first_edit_turn == 0 {
                *first_edit_turn = current_turn;
            }
        }
        "Glob" => {
            if let Some(input) = input {
                if let Some(pattern) = input.get("pattern").and_then(|p| p.as_str()) {
                    detail.args.push(pattern.to_string());
                }
            }
        }
        "Grep" => {
            if let Some(input) = input {
                if let Some(pattern) = input.get("pattern").and_then(|p| p.as_str()) {
                    detail.args.push(pattern.to_string());
                }
            }
        }
        "Bash" => {
            if let Some(input) = input {
                if let Some(command) = input.get("command").and_then(|c| c.as_str()) {
                    // Truncate long commands
                    let truncated = if command.len() > 200 {
                        format!("{}...", &command[..197])
                    } else {
                        command.to_string()
                    };
                    detail.args.push(truncated);
                }
            }
        }
        _ => {
            // Track fmm MCP tool calls (tools starting with fmm_ or mcp__fmm)
            if name.starts_with("fmm_") || name.starts_with("mcp__fmm") {
                metrics.fmm_usage.mcp_tool_calls += 1;
                metrics.fmm_usage.fmm_tool_names.push(name.to_string());
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dur(ms: u64) -> Duration {
        Duration::from_millis(ms)
    }

    #[test]
    fn parse_successful_result() {
        let output = r#"{"type":"assistant","message":{"content":[{"type":"text","text":"Fixed the bug"}]}}
{"type":"result","is_error":false,"result":"Done","total_cost_usd":0.05,"num_turns":3,"usage":{"input_tokens":1000,"output_tokens":500,"cache_read_input_tokens":100,"cache_creation_input_tokens":50},"duration_ms":5000}"#;
        let parsed = parse_stream_json(output, dur(0)).unwrap();
        assert!(parsed.metrics.success);
        assert_eq!(parsed.response_text, "Fixed the bug");
        assert!((parsed.metrics.cost_usd - 0.05).abs() < f64::EPSILON);
        assert_eq!(parsed.metrics.turns, 3);
        assert_eq!(parsed.metrics.input_tokens, 1000);
        assert_eq!(parsed.metrics.output_tokens, 500);
        assert_eq!(parsed.metrics.cache_read_tokens, 100);
        assert_eq!(parsed.metrics.cache_creation_tokens, 50);
        assert_eq!(parsed.metrics.duration_ms, 5000);
    }

    #[test]
    fn parse_error_result() {
        let output = r#"{"type":"result","is_error":true,"subtype":"budget_exceeded","total_cost_usd":5.0,"num_turns":30,"usage":{"input_tokens":10000,"output_tokens":5000},"duration_ms":60000}"#;
        let parsed = parse_stream_json(output, dur(0)).unwrap();
        assert!(!parsed.metrics.success);
        assert_eq!(parsed.metrics.error.as_deref(), Some("budget_exceeded"));
        assert!((parsed.metrics.cost_usd - 5.0).abs() < f64::EPSILON);
        assert_eq!(parsed.metrics.turns, 30);
    }

    #[test]
    fn parse_empty_output() {
        let parsed = parse_stream_json("", dur(0)).unwrap();
        assert!(!parsed.metrics.success);
        assert_eq!(parsed.metrics.turns, 0);
    }

    #[test]
    fn parse_malformed_lines_skipped() {
        let output = "not json\n{broken\n{\"type\":\"result\",\"is_error\":false,\"total_cost_usd\":0.01,\"num_turns\":1,\"usage\":{\"input_tokens\":10,\"output_tokens\":5},\"duration_ms\":100}";
        let parsed = parse_stream_json(output, dur(100)).unwrap();
        assert!(parsed.metrics.success);
        assert_eq!(parsed.metrics.turns, 1);
        assert_eq!(parsed.metrics.input_tokens, 10);
    }

    #[test]
    fn parse_tool_calls() {
        let output = r#"{"type":"assistant","message":{"content":[{"type":"tool_use","name":"Read","input":{"file_path":"src/main.rs"}},{"type":"tool_use","name":"Glob","input":{"pattern":"**/*.ts"}}]}}
{"type":"result","is_error":false,"result":"done","usage":{"input_tokens":500,"output_tokens":200,"cache_read_input_tokens":50},"total_cost_usd":0.005,"num_turns":1,"duration_ms":1200}"#;

        let parsed = parse_stream_json(output, dur(1200)).unwrap();
        assert_eq!(parsed.metrics.tool_calls, 2);
        assert_eq!(parsed.metrics.tools_by_name["Read"], 1);
        assert_eq!(parsed.metrics.tools_by_name["Glob"], 1);
        assert_eq!(parsed.metrics.read_calls, 1);
        assert_eq!(parsed.metrics.files_accessed, vec!["src/main.rs"]);

        // New: tool details
        assert_eq!(parsed.metrics.tool_details["Read"].count, 1);
        assert_eq!(
            parsed.metrics.tool_details["Read"].args,
            vec!["src/main.rs"]
        );
        assert_eq!(parsed.metrics.tool_details["Glob"].args, vec!["**/*.ts"]);
    }

    #[test]
    fn parse_view_tracked_as_read() {
        let output = r#"{"type":"assistant","message":{"content":[{"type":"tool_use","name":"View","input":{"path":"src/lib.rs"}}]}}
{"type":"result","is_error":false,"usage":{"input_tokens":10,"output_tokens":5},"total_cost_usd":0.001,"num_turns":1,"duration_ms":100}"#;

        let parsed = parse_stream_json(output, dur(100)).unwrap();
        assert_eq!(parsed.metrics.read_calls, 1);
        assert_eq!(parsed.metrics.files_accessed, vec!["src/lib.rs"]);
    }

    #[test]
    fn fallback_duration_used_when_no_duration_ms() {
        let output = r#"{"type":"result","is_error":false,"total_cost_usd":0.01,"num_turns":1,"usage":{"input_tokens":10,"output_tokens":5}}"#;
        let parsed = parse_stream_json(output, dur(9999)).unwrap();
        assert_eq!(parsed.metrics.duration_ms, 9999);
    }

    #[test]
    fn navigation_efficiency_exploration_then_edit() {
        // Turn 1: Read (exploration), Turn 2: Edit (implementation)
        let output = r#"{"type":"assistant","message":{"content":[{"type":"tool_use","name":"Read","input":{"file_path":"src/a.rs"}}]}}
{"type":"assistant","message":{"content":[{"type":"tool_use","name":"Read","input":{"file_path":"src/b.rs"}}]}}
{"type":"assistant","message":{"content":[{"type":"tool_use","name":"Edit","input":{"file_path":"src/a.rs","old_string":"x","new_string":"y"}}]}}
{"type":"assistant","message":{"content":[{"type":"tool_use","name":"Edit","input":{"file_path":"src/b.rs","old_string":"x","new_string":"y"}}]}}
{"type":"result","is_error":false,"usage":{"input_tokens":100,"output_tokens":50},"total_cost_usd":0.01,"num_turns":4,"duration_ms":1000}"#;

        let parsed = parse_stream_json(output, dur(1000)).unwrap();
        let nav = &parsed.metrics.navigation;
        assert_eq!(nav.unique_files_read, 2);
        assert_eq!(nav.unique_files_edited, 2);
        assert_eq!(nav.first_edit_turn, 3);
        assert_eq!(nav.exploration_turns, 2);
        assert_eq!(nav.implementation_turns, 2);
    }

    #[test]
    fn navigation_no_edits() {
        let output = r#"{"type":"assistant","message":{"content":[{"type":"tool_use","name":"Read","input":{"file_path":"src/a.rs"}}]}}
{"type":"result","is_error":false,"usage":{"input_tokens":10,"output_tokens":5},"total_cost_usd":0.001,"num_turns":1,"duration_ms":100}"#;

        let parsed = parse_stream_json(output, dur(100)).unwrap();
        let nav = &parsed.metrics.navigation;
        assert_eq!(nav.first_edit_turn, 0);
        assert_eq!(nav.exploration_turns, 1);
        assert_eq!(nav.implementation_turns, 0);
    }

    #[test]
    fn fmm_sidecar_reads_tracked() {
        let output = r#"{"type":"assistant","message":{"content":[{"type":"tool_use","name":"Read","input":{"file_path":"src/main.rs.fmm"}},{"type":"tool_use","name":"Read","input":{"file_path":"src/lib.rs.fmm"}}]}}
{"type":"result","is_error":false,"usage":{"input_tokens":10,"output_tokens":5},"total_cost_usd":0.001,"num_turns":1,"duration_ms":100}"#;

        let parsed = parse_stream_json(output, dur(100)).unwrap();
        assert_eq!(parsed.metrics.fmm_usage.sidecars_read, 2);
    }

    #[test]
    fn fmm_mcp_tools_tracked() {
        let output = r#"{"type":"assistant","message":{"content":[{"type":"tool_use","name":"fmm_lookup_export","input":{"name":"createStore"}},{"type":"tool_use","name":"mcp__fmm__search","input":{}}]}}
{"type":"result","is_error":false,"usage":{"input_tokens":10,"output_tokens":5},"total_cost_usd":0.001,"num_turns":1,"duration_ms":100}"#;

        let parsed = parse_stream_json(output, dur(100)).unwrap();
        assert_eq!(parsed.metrics.fmm_usage.mcp_tool_calls, 2);
        assert!(parsed
            .metrics
            .fmm_usage
            .fmm_tool_names
            .contains(&"fmm_lookup_export".to_string()));
        assert!(parsed
            .metrics
            .fmm_usage
            .fmm_tool_names
            .contains(&"mcp__fmm__search".to_string()));
    }

    #[test]
    fn edit_and_write_tracked_in_details() {
        let output = r#"{"type":"assistant","message":{"content":[{"type":"tool_use","name":"Edit","input":{"file_path":"src/main.rs","old_string":"a","new_string":"b"}},{"type":"tool_use","name":"Write","input":{"file_path":"src/new.rs","content":"fn main() {}"}}]}}
{"type":"result","is_error":false,"usage":{"input_tokens":10,"output_tokens":5},"total_cost_usd":0.001,"num_turns":1,"duration_ms":100}"#;

        let parsed = parse_stream_json(output, dur(100)).unwrap();
        assert_eq!(parsed.metrics.tool_details["Edit"].count, 1);
        assert_eq!(
            parsed.metrics.tool_details["Edit"].args,
            vec!["src/main.rs"]
        );
        assert_eq!(parsed.metrics.tool_details["Write"].count, 1);
        assert_eq!(
            parsed.metrics.tool_details["Write"].args,
            vec!["src/new.rs"]
        );
        assert_eq!(parsed.metrics.navigation.unique_files_edited, 2);
    }

    #[test]
    fn bash_commands_tracked() {
        let output = r#"{"type":"assistant","message":{"content":[{"type":"tool_use","name":"Bash","input":{"command":"npm test"}}]}}
{"type":"result","is_error":false,"usage":{"input_tokens":10,"output_tokens":5},"total_cost_usd":0.001,"num_turns":1,"duration_ms":100}"#;

        let parsed = parse_stream_json(output, dur(100)).unwrap();
        assert_eq!(parsed.metrics.tool_details["Bash"].count, 1);
        assert_eq!(parsed.metrics.tool_details["Bash"].args, vec!["npm test"]);
    }

    #[test]
    fn grep_patterns_tracked() {
        let output = r#"{"type":"assistant","message":{"content":[{"type":"tool_use","name":"Grep","input":{"pattern":"createStore"}}]}}
{"type":"result","is_error":false,"usage":{"input_tokens":10,"output_tokens":5},"total_cost_usd":0.001,"num_turns":1,"duration_ms":100}"#;

        let parsed = parse_stream_json(output, dur(100)).unwrap();
        assert_eq!(
            parsed.metrics.tool_details["Grep"].args,
            vec!["createStore"]
        );
    }
}
