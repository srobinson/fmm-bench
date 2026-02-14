//! Claude CLI runner with instrumentation for benchmarking

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::Path;
use std::process::Command;
use std::time::Instant;

use crate::metrics;
use crate::tasks::Task;

/// Result of a single benchmark run
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RunResult {
    pub task_id: String,
    pub variant: String,
    pub tool_calls: u32,
    pub tools_by_name: HashMap<String, u32>,
    pub files_accessed: Vec<String>,
    pub read_calls: u32,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cache_read_tokens: u64,
    pub total_cost_usd: f64,
    pub duration_ms: u64,
    pub num_turns: u32,
    pub response: String,
    pub success: bool,
    pub error: Option<String>,

    /// Per-tool detail with args (files, patterns, commands).
    #[serde(default)]
    pub tool_details: HashMap<String, metrics::ToolDetail>,
    /// Navigation efficiency metrics.
    #[serde(default)]
    pub navigation: metrics::NavigationMetrics,
    /// FMM-specific usage tracking.
    #[serde(default)]
    pub fmm_usage: metrics::FmmUsage,
}

impl RunResult {
    /// Create a RunResult from shared RunMetrics plus context identifiers.
    fn from_metrics(
        m: metrics::RunMetrics,
        response: String,
        task_id: &str,
        variant: &str,
    ) -> Self {
        Self {
            task_id: task_id.to_string(),
            variant: variant.to_string(),
            tool_calls: m.tool_calls,
            tools_by_name: m.tools_by_name,
            files_accessed: m.files_accessed,
            read_calls: m.read_calls,
            input_tokens: m.input_tokens,
            output_tokens: m.output_tokens,
            cache_read_tokens: m.cache_read_tokens,
            total_cost_usd: m.cost_usd,
            duration_ms: m.duration_ms,
            num_turns: m.turns,
            response,
            success: m.success,
            error: m.error,
            tool_details: m.tool_details,
            navigation: m.navigation,
            fmm_usage: m.fmm_usage,
        }
    }
}

/// Claude CLI runner with instrumentation
pub struct ClaudeRunner {
    allowed_tools: Vec<String>,
    model: String,
    skip_permissions: bool,
    enable_local_settings: bool,
}

impl Default for ClaudeRunner {
    fn default() -> Self {
        Self::new()
    }
}

impl ClaudeRunner {
    pub fn new() -> Self {
        Self {
            allowed_tools: vec![
                "Read".to_string(),
                "Glob".to_string(),
                "Grep".to_string(),
                "LS".to_string(),
                "Edit".to_string(),
                "Write".to_string(),
                "Bash".to_string(),
            ],
            model: "sonnet".to_string(),
            skip_permissions: true,
            enable_local_settings: false,
        }
    }

    /// Create a runner with local settings enabled (skill + MCP from workspace).
    pub fn with_local_settings() -> Self {
        Self {
            enable_local_settings: true,
            ..Self::new()
        }
    }

    /// Set the model for this runner.
    pub fn set_model(&mut self, model: &str) {
        self.model = model.to_string();
    }

    const MAX_PROMPT_SIZE: usize = 100 * 1024;
    const MAX_CONTEXT_SIZE: usize = 500 * 1024;

    /// Run a task and collect metrics
    pub fn run_task(
        &self,
        task: &Task,
        working_dir: &Path,
        variant: &str,
        fmm_context: Option<&str>,
    ) -> Result<RunResult> {
        if task.prompt.len() > Self::MAX_PROMPT_SIZE {
            anyhow::bail!(
                "Task prompt exceeds size limit ({} > {} bytes)",
                task.prompt.len(),
                Self::MAX_PROMPT_SIZE
            );
        }
        if let Some(ctx) = fmm_context {
            if ctx.len() > Self::MAX_CONTEXT_SIZE {
                anyhow::bail!(
                    "FMM context exceeds size limit ({} > {} bytes)",
                    ctx.len(),
                    Self::MAX_CONTEXT_SIZE
                );
            }
        }

        let start = Instant::now();

        let mut cmd = Command::new("claude");

        cmd.arg("-p").arg(&task.prompt);
        cmd.arg("--output-format").arg("stream-json");
        cmd.arg("--verbose");
        cmd.arg("--max-turns").arg(task.max_turns.to_string());
        cmd.arg("--max-budget-usd")
            .arg(task.max_budget_usd.to_string());
        cmd.arg("--model").arg(&self.model);

        if !self.allowed_tools.is_empty() {
            cmd.arg("--allowedTools").arg(self.allowed_tools.join(","));
        }

        if self.enable_local_settings {
            cmd.arg("--setting-sources").arg("local");
        } else {
            cmd.arg("--setting-sources").arg("");
        }

        if let Some(context) = fmm_context {
            cmd.arg("--append-system-prompt").arg(context);
        }

        if self.skip_permissions {
            cmd.arg("--dangerously-skip-permissions");
        }

        cmd.arg("--no-session-persistence");
        cmd.current_dir(working_dir);

        let output = cmd.output().context("Failed to execute claude CLI")?;

        let duration = start.elapsed();
        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);
        let cli_success = output.status.success();

        if !cli_success && stdout.is_empty() {
            return Ok(RunResult::from_metrics(
                metrics::RunMetrics {
                    duration_ms: duration.as_millis() as u64,
                    error: Some(stderr.to_string()),
                    ..Default::default()
                },
                String::new(),
                &task.id,
                variant,
            ));
        }

        let parsed = metrics::parse_stream_json(&stdout, duration)?;
        let mut result =
            RunResult::from_metrics(parsed.metrics, parsed.response_text, &task.id, variant);

        if !cli_success {
            result.success = false;
            if result.error.is_none() {
                result.error = Some(format!(
                    "CLI exited with status {}",
                    output.status.code().unwrap_or(-1)
                ));
            }
        }
        Ok(result)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_runner_creation() {
        let runner = ClaudeRunner::new();
        assert!(!runner.allowed_tools.is_empty());
    }

    fn dur(ms: u64) -> std::time::Duration {
        std::time::Duration::from_millis(ms)
    }

    #[test]
    fn test_parse_stream_json_tool_calls() {
        let output = r#"{"type":"assistant","message":{"content":[{"type":"tool_use","name":"Read","input":{"file_path":"src/main.rs"}},{"type":"tool_use","name":"Glob","input":{"pattern":"**/*.ts"}}]}}
{"type":"result","is_error":false,"result":"done","usage":{"input_tokens":500,"output_tokens":200,"cache_read_input_tokens":50},"total_cost_usd":0.005,"num_turns":1,"duration_ms":1200}"#;

        let parsed = metrics::parse_stream_json(output, dur(1200)).unwrap();
        let result =
            RunResult::from_metrics(parsed.metrics, parsed.response_text, "test", "control");

        assert_eq!(result.tool_calls, 2);
        assert_eq!(result.tools_by_name["Read"], 1);
        assert_eq!(result.tools_by_name["Glob"], 1);
        assert_eq!(result.read_calls, 1);
        assert_eq!(result.files_accessed, vec!["src/main.rs"]);
        assert_eq!(result.input_tokens, 500);
        assert_eq!(result.output_tokens, 200);
        assert_eq!(result.cache_read_tokens, 50);
        assert!((result.total_cost_usd - 0.005).abs() < f64::EPSILON);
        assert!(result.success);
        assert!(result.error.is_none());
    }

    #[test]
    fn test_parse_stream_json_multiple_tool_types() {
        let output = r#"{"type":"assistant","message":{"content":[{"type":"tool_use","name":"Read","input":{"file_path":"a.rs"}},{"type":"tool_use","name":"Read","input":{"file_path":"b.rs"}},{"type":"tool_use","name":"Grep","input":{"pattern":"foo"}}]}}
{"type":"assistant","message":{"content":[{"type":"tool_use","name":"Glob","input":{"pattern":"*.rs"}}]}}
{"type":"result","is_error":false,"usage":{"input_tokens":100,"output_tokens":50},"total_cost_usd":0.001,"num_turns":2,"duration_ms":500}"#;

        let parsed = metrics::parse_stream_json(output, dur(500)).unwrap();
        let result = RunResult::from_metrics(parsed.metrics, parsed.response_text, "multi", "fmm");

        assert_eq!(result.tool_calls, 4);
        assert_eq!(result.tools_by_name["Read"], 2);
        assert_eq!(result.tools_by_name["Grep"], 1);
        assert_eq!(result.tools_by_name["Glob"], 1);
        assert_eq!(result.read_calls, 2);
        assert_eq!(result.files_accessed.len(), 2);
        assert!(result.files_accessed.contains(&"a.rs".to_string()));
        assert!(result.files_accessed.contains(&"b.rs".to_string()));
        assert_eq!(result.num_turns, 2);
    }

    #[test]
    fn test_parse_stream_json_error_result() {
        let output = r#"{"type":"result","is_error":true,"subtype":"budget_exceeded","usage":{"input_tokens":100,"output_tokens":50},"total_cost_usd":2.0,"num_turns":5,"duration_ms":10000}"#;

        let parsed = metrics::parse_stream_json(output, dur(10000)).unwrap();
        let result =
            RunResult::from_metrics(parsed.metrics, parsed.response_text, "fail", "control");

        assert!(!result.success);
        assert_eq!(result.error.as_deref(), Some("budget_exceeded"));
        assert!((result.total_cost_usd - 2.0).abs() < f64::EPSILON);
    }

    #[test]
    fn test_parse_stream_json_no_result_event() {
        let output =
            r#"{"type":"assistant","message":{"content":[{"type":"text","text":"Hello"}]}}"#;

        let parsed = metrics::parse_stream_json(output, dur(100)).unwrap();
        let result =
            RunResult::from_metrics(parsed.metrics, parsed.response_text, "noresult", "control");

        assert!(!result.success);
        assert_eq!(result.tool_calls, 0);
    }

    #[test]
    fn test_parse_stream_json_malformed_lines() {
        let output = "not valid json\n{broken\n\n{\"type\":\"result\",\"is_error\":false,\"usage\":{\"input_tokens\":10,\"output_tokens\":5},\"total_cost_usd\":0.001,\"num_turns\":1,\"duration_ms\":100}";

        let parsed = metrics::parse_stream_json(output, dur(100)).unwrap();
        let result =
            RunResult::from_metrics(parsed.metrics, parsed.response_text, "malformed", "control");

        assert!(result.success);
        assert_eq!(result.input_tokens, 10);
    }

    #[test]
    fn test_parse_stream_json_text_response() {
        let output = r#"{"type":"assistant","message":{"content":[{"type":"text","text":"The entry point is main.rs"}]}}
{"type":"result","is_error":false,"usage":{"input_tokens":50,"output_tokens":30},"total_cost_usd":0.001,"num_turns":1,"duration_ms":100}"#;

        let parsed = metrics::parse_stream_json(output, dur(100)).unwrap();
        let result = RunResult::from_metrics(parsed.metrics, parsed.response_text, "text", "fmm");

        assert_eq!(result.response, "The entry point is main.rs");
        assert!(result.success);
    }

    #[test]
    fn test_parse_stream_json_empty_output() {
        let parsed = metrics::parse_stream_json("", dur(0)).unwrap();
        let result =
            RunResult::from_metrics(parsed.metrics, parsed.response_text, "empty", "control");

        assert!(!result.success);
        assert_eq!(result.tool_calls, 0);
        assert_eq!(result.input_tokens, 0);
    }

    #[test]
    fn test_parse_stream_json_view_tracked_as_read() {
        let output = r#"{"type":"assistant","message":{"content":[{"type":"tool_use","name":"View","input":{"path":"src/lib.rs"}}]}}
{"type":"result","is_error":false,"usage":{"input_tokens":10,"output_tokens":5},"total_cost_usd":0.001,"num_turns":1,"duration_ms":100}"#;

        let parsed = metrics::parse_stream_json(output, dur(100)).unwrap();
        let result =
            RunResult::from_metrics(parsed.metrics, parsed.response_text, "view", "control");

        assert_eq!(result.read_calls, 1);
        assert_eq!(result.files_accessed, vec!["src/lib.rs"]);
    }

    #[test]
    fn test_prompt_size_limit() {
        let runner = ClaudeRunner::new();
        let big_prompt = "x".repeat(ClaudeRunner::MAX_PROMPT_SIZE + 1);
        let task = crate::tasks::Task {
            id: "big".to_string(),
            name: "Big".to_string(),
            prompt: big_prompt,
            category: crate::tasks::TaskCategory::Exploration,
            expected_patterns: vec![],
            max_turns: 1,
            max_budget_usd: 0.01,
        };

        let err = runner
            .run_task(&task, Path::new("/tmp"), "control", None)
            .unwrap_err();
        assert!(err.to_string().contains("prompt exceeds size limit"));
    }

    #[test]
    fn test_context_size_limit() {
        let runner = ClaudeRunner::new();
        let task = crate::tasks::Task {
            id: "ctx".to_string(),
            name: "Ctx".to_string(),
            prompt: "small prompt".to_string(),
            category: crate::tasks::TaskCategory::Exploration,
            expected_patterns: vec![],
            max_turns: 1,
            max_budget_usd: 0.01,
        };
        let big_context = "y".repeat(ClaudeRunner::MAX_CONTEXT_SIZE + 1);

        let err = runner
            .run_task(&task, Path::new("/tmp"), "fmm", Some(&big_context))
            .unwrap_err();
        assert!(err.to_string().contains("FMM context exceeds size limit"));
    }
}
