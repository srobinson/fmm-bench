//! Shared metrics types and stream-json parser for Claude CLI output.
//!
//! Used by both `gh::runner` (issue fixing) and `compare::runner` (benchmarking).

use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::time::Duration;

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
                if let Some(message) = data.get("message") {
                    if let Some(content) = message.get("content").and_then(|c| c.as_array()) {
                        for item in content {
                            match item.get("type").and_then(|t| t.as_str()) {
                                Some("tool_use") => {
                                    metrics.tool_calls += 1;

                                    if let Some(name) = item.get("name").and_then(|n| n.as_str()) {
                                        *metrics
                                            .tools_by_name
                                            .entry(name.to_string())
                                            .or_insert(0) += 1;

                                        if name == "Read" || name == "View" {
                                            metrics.read_calls += 1;
                                            if let Some(input) = item.get("input") {
                                                if let Some(path) = input
                                                    .get("file_path")
                                                    .or(input.get("path"))
                                                    .and_then(|p| p.as_str())
                                                {
                                                    metrics.files_accessed.push(path.to_string());
                                                }
                                            }
                                        }
                                    }
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

    Ok(ParsedOutput {
        metrics,
        response_text,
    })
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
}
