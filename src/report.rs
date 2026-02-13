//! Comparison report generation - JSON and Markdown formats

use colored::Colorize;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::Path;

use crate::runner::RunResult;
use crate::tasks::Task;

/// Format for report output
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum ReportFormat {
    Json,
    Markdown,
    #[default]
    Both,
}

/// Complete comparison report
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ComparisonReport {
    /// Job ID
    pub job_id: String,
    /// Repository URL
    pub repo_url: String,
    /// Commit SHA
    pub commit_sha: String,
    /// Branch
    pub branch: String,
    /// Timestamp
    pub timestamp: String,
    /// Task results
    pub task_results: Vec<TaskComparison>,
    /// Aggregated metrics
    pub summary: ComparisonSummary,
}

/// Comparison for a single task
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskComparison {
    /// Task ID
    pub task_id: String,
    /// Task name
    pub task_name: String,
    /// Control variant result
    pub control: RunResult,
    /// FMM variant result
    pub fmm: RunResult,
    /// Calculated savings
    pub savings: TaskSavings,
}

/// Savings metrics for a task
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskSavings {
    /// Tool call reduction percentage
    pub tool_calls_reduction_pct: f64,
    /// Read call reduction percentage
    pub read_calls_reduction_pct: f64,
    /// Token reduction percentage
    pub tokens_reduction_pct: f64,
    /// Cost reduction percentage
    pub cost_reduction_pct: f64,
    /// Duration reduction percentage
    pub duration_reduction_pct: f64,
}

/// Summary of comparison results
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ComparisonSummary {
    /// Total tasks run
    pub tasks_run: u32,
    /// Tasks where FMM was better
    pub fmm_wins: u32,
    /// Tasks where control was better
    pub control_wins: u32,
    /// Tasks with equal performance
    pub ties: u32,
    /// Aggregate control metrics
    pub control_totals: AggregateMetrics,
    /// Aggregate FMM metrics
    pub fmm_totals: AggregateMetrics,
    /// Overall savings
    pub overall_savings: OverallSavings,
}

/// Aggregated metrics across all tasks
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AggregateMetrics {
    pub total_tool_calls: u32,
    pub total_read_calls: u32,
    pub total_input_tokens: u64,
    pub total_output_tokens: u64,
    pub total_cost_usd: f64,
    pub total_duration_ms: u64,
    pub avg_tool_calls: f64,
    pub avg_cost_usd: f64,
}

/// Overall savings summary
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OverallSavings {
    pub tool_calls_reduction_pct: f64,
    pub read_calls_reduction_pct: f64,
    pub tokens_reduction_pct: f64,
    pub cost_reduction_pct: f64,
    pub duration_reduction_pct: f64,
}

impl ComparisonReport {
    /// Create a new report from task results
    pub fn new(
        job_id: String,
        repo_url: String,
        commit_sha: String,
        branch: String,
        results: Vec<(Task, RunResult, RunResult)>,
    ) -> Self {
        let timestamp = chrono::Utc::now().to_rfc3339();

        let task_results: Vec<TaskComparison> = results
            .into_iter()
            .map(|(task, control, fmm)| {
                let savings = calculate_savings(&control, &fmm);
                TaskComparison {
                    task_id: task.id,
                    task_name: task.name,
                    control,
                    fmm,
                    savings,
                }
            })
            .collect();

        let summary = Self::calculate_summary(&task_results);

        Self {
            job_id,
            repo_url,
            commit_sha,
            branch,
            timestamp,
            task_results,
            summary,
        }
    }

    fn calculate_summary(task_results: &[TaskComparison]) -> ComparisonSummary {
        let tasks_run = task_results.len() as u32;

        let mut fmm_wins = 0u32;
        let mut control_wins = 0u32;
        let mut ties = 0u32;

        let mut control_totals = AggregateMetrics {
            total_tool_calls: 0,
            total_read_calls: 0,
            total_input_tokens: 0,
            total_output_tokens: 0,
            total_cost_usd: 0.0,
            total_duration_ms: 0,
            avg_tool_calls: 0.0,
            avg_cost_usd: 0.0,
        };

        let mut fmm_totals = AggregateMetrics {
            total_tool_calls: 0,
            total_read_calls: 0,
            total_input_tokens: 0,
            total_output_tokens: 0,
            total_cost_usd: 0.0,
            total_duration_ms: 0,
            avg_tool_calls: 0.0,
            avg_cost_usd: 0.0,
        };

        for result in task_results {
            // Determine winner (fewer tool calls = better)
            match result.control.tool_calls.cmp(&result.fmm.tool_calls) {
                std::cmp::Ordering::Greater => fmm_wins += 1,
                std::cmp::Ordering::Less => control_wins += 1,
                std::cmp::Ordering::Equal => ties += 1,
            }

            // Aggregate control metrics
            control_totals.total_tool_calls += result.control.tool_calls;
            control_totals.total_read_calls += result.control.read_calls;
            control_totals.total_input_tokens += result.control.input_tokens;
            control_totals.total_output_tokens += result.control.output_tokens;
            control_totals.total_cost_usd += result.control.total_cost_usd;
            control_totals.total_duration_ms += result.control.duration_ms;

            // Aggregate FMM metrics
            fmm_totals.total_tool_calls += result.fmm.tool_calls;
            fmm_totals.total_read_calls += result.fmm.read_calls;
            fmm_totals.total_input_tokens += result.fmm.input_tokens;
            fmm_totals.total_output_tokens += result.fmm.output_tokens;
            fmm_totals.total_cost_usd += result.fmm.total_cost_usd;
            fmm_totals.total_duration_ms += result.fmm.duration_ms;
        }

        // Calculate averages
        if tasks_run > 0 {
            control_totals.avg_tool_calls =
                control_totals.total_tool_calls as f64 / tasks_run as f64;
            control_totals.avg_cost_usd = control_totals.total_cost_usd / tasks_run as f64;
            fmm_totals.avg_tool_calls = fmm_totals.total_tool_calls as f64 / tasks_run as f64;
            fmm_totals.avg_cost_usd = fmm_totals.total_cost_usd / tasks_run as f64;
        }

        // Calculate overall savings
        let overall_savings = OverallSavings {
            tool_calls_reduction_pct: calculate_reduction_pct(
                control_totals.total_tool_calls as f64,
                fmm_totals.total_tool_calls as f64,
            ),
            read_calls_reduction_pct: calculate_reduction_pct(
                control_totals.total_read_calls as f64,
                fmm_totals.total_read_calls as f64,
            ),
            tokens_reduction_pct: calculate_reduction_pct(
                (control_totals.total_input_tokens + control_totals.total_output_tokens) as f64,
                (fmm_totals.total_input_tokens + fmm_totals.total_output_tokens) as f64,
            ),
            cost_reduction_pct: calculate_reduction_pct(
                control_totals.total_cost_usd,
                fmm_totals.total_cost_usd,
            ),
            duration_reduction_pct: calculate_reduction_pct(
                control_totals.total_duration_ms as f64,
                fmm_totals.total_duration_ms as f64,
            ),
        };

        ComparisonSummary {
            tasks_run,
            fmm_wins,
            control_wins,
            ties,
            control_totals,
            fmm_totals,
            overall_savings,
        }
    }

    /// Print summary to stdout
    pub fn print_summary(&self) {
        let s = &self.summary;

        println!("\n{}", "Summary".yellow().bold());
        println!(
            "  Tasks run: {} | FMM wins: {} | Control wins: {} | Ties: {}",
            s.tasks_run.to_string().white().bold(),
            s.fmm_wins.to_string().green().bold(),
            s.control_wins.to_string().red(),
            s.ties.to_string().dimmed()
        );

        println!("\n{}", "Tool Calls".yellow().bold());
        println!(
            "  Control: {} | FMM: {} | Reduction: {}",
            s.control_totals.total_tool_calls.to_string().white(),
            s.fmm_totals.total_tool_calls.to_string().green(),
            format!("{:.1}%", s.overall_savings.tool_calls_reduction_pct)
                .green()
                .bold()
        );

        println!("\n{}", "Cost".yellow().bold());
        println!(
            "  Control: ${:.4} | FMM: ${:.4} | Savings: {}",
            s.control_totals.total_cost_usd,
            s.fmm_totals.total_cost_usd,
            format!("{:.1}%", s.overall_savings.cost_reduction_pct)
                .green()
                .bold()
        );

        println!("\n{}", "Per Task Breakdown".yellow().bold());
        println!(
            "  {:20} {:>10} {:>10} {:>12}",
            "Task".dimmed(),
            "Control".dimmed(),
            "FMM".dimmed(),
            "Reduction".dimmed()
        );
        println!("  {}", "-".repeat(54).dimmed());

        for task in &self.task_results {
            let reduction = if task.savings.tool_calls_reduction_pct > 0.0 {
                format!("{:.1}%", task.savings.tool_calls_reduction_pct)
                    .green()
                    .to_string()
            } else if task.savings.tool_calls_reduction_pct < 0.0 {
                format!("{:.1}%", task.savings.tool_calls_reduction_pct)
                    .red()
                    .to_string()
            } else {
                "0%".dimmed().to_string()
            };

            println!(
                "  {:20} {:>10} {:>10} {:>12}",
                truncate(&task.task_name, 20),
                task.control.tool_calls,
                task.fmm.tool_calls,
                reduction
            );
        }
    }

    /// Save report to file(s)
    pub fn save(&self, output_dir: &Path, format: ReportFormat) -> anyhow::Result<Vec<String>> {
        fs::create_dir_all(output_dir)?;
        let mut saved_files = vec![];

        if format == ReportFormat::Json || format == ReportFormat::Both {
            let json_path = output_dir.join(format!("{}.json", self.job_id));
            let json = serde_json::to_string_pretty(self)?;
            fs::write(&json_path, json)?;
            saved_files.push(json_path.display().to_string());
        }

        if format == ReportFormat::Markdown || format == ReportFormat::Both {
            let md_path = output_dir.join(format!("{}.md", self.job_id));
            let markdown = self.to_markdown();
            fs::write(&md_path, markdown)?;
            saved_files.push(md_path.display().to_string());
        }

        Ok(saved_files)
    }

    /// Generate markdown report
    pub fn to_markdown(&self) -> String {
        let mut md = String::new();
        let s = &self.summary;

        md.push_str(&format!("# FMM Comparison Report: {}\n\n", self.repo_url));
        md.push_str(&format!("**Job ID:** {}\n", self.job_id));
        md.push_str(&format!("**Commit:** {}\n", self.commit_sha));
        md.push_str(&format!("**Branch:** {}\n", self.branch));
        md.push_str(&format!("**Timestamp:** {}\n\n", self.timestamp));

        md.push_str("## Summary\n\n");
        md.push_str("| Metric | Control | FMM | Reduction |\n");
        md.push_str("|--------|---------|-----|----------|\n");
        md.push_str(&format!(
            "| Tool Calls | {} | {} | {:.1}% |\n",
            s.control_totals.total_tool_calls,
            s.fmm_totals.total_tool_calls,
            s.overall_savings.tool_calls_reduction_pct
        ));
        md.push_str(&format!(
            "| Read Calls | {} | {} | {:.1}% |\n",
            s.control_totals.total_read_calls,
            s.fmm_totals.total_read_calls,
            s.overall_savings.read_calls_reduction_pct
        ));
        md.push_str(&format!(
            "| Cost (USD) | ${:.4} | ${:.4} | {:.1}% |\n",
            s.control_totals.total_cost_usd,
            s.fmm_totals.total_cost_usd,
            s.overall_savings.cost_reduction_pct
        ));
        md.push_str(&format!(
            "| Duration (ms) | {} | {} | {:.1}% |\n\n",
            s.control_totals.total_duration_ms,
            s.fmm_totals.total_duration_ms,
            s.overall_savings.duration_reduction_pct
        ));

        let win_percentage = if s.tasks_run > 0 {
            (s.fmm_wins as f64 / s.tasks_run as f64) * 100.0
        } else {
            0.0
        };
        md.push_str(&format!(
            "**FMM Wins:** {} / {} tasks ({:.0}%)\n\n",
            s.fmm_wins, s.tasks_run, win_percentage
        ));

        md.push_str("## Task Details\n\n");

        for task in &self.task_results {
            md.push_str(&format!("### {}\n\n", task.task_name));
            md.push_str("| Metric | Control | FMM |\n");
            md.push_str("|--------|---------|-----|\n");
            md.push_str(&format!(
                "| Tool Calls | {} | {} |\n",
                task.control.tool_calls, task.fmm.tool_calls
            ));
            md.push_str(&format!(
                "| Read Calls | {} | {} |\n",
                task.control.read_calls, task.fmm.read_calls
            ));
            md.push_str(&format!(
                "| Cost | ${:.4} | ${:.4} |\n",
                task.control.total_cost_usd, task.fmm.total_cost_usd
            ));
            md.push_str(&format!(
                "| Duration | {}ms | {}ms |\n\n",
                task.control.duration_ms, task.fmm.duration_ms
            ));

            if !task.control.tools_by_name.is_empty() {
                md.push_str("**Control Tools Used:**\n");
                for (tool, count) in &task.control.tools_by_name {
                    md.push_str(&format!("- {}: {}\n", tool, count));
                }
                md.push('\n');
            }

            if !task.fmm.tools_by_name.is_empty() {
                md.push_str("**FMM Tools Used:**\n");
                for (tool, count) in &task.fmm.tools_by_name {
                    md.push_str(&format!("- {}: {}\n", tool, count));
                }
                md.push('\n');
            }
        }

        md
    }
}

fn calculate_savings(control: &RunResult, fmm: &RunResult) -> TaskSavings {
    TaskSavings {
        tool_calls_reduction_pct: calculate_reduction_pct(
            control.tool_calls as f64,
            fmm.tool_calls as f64,
        ),
        read_calls_reduction_pct: calculate_reduction_pct(
            control.read_calls as f64,
            fmm.read_calls as f64,
        ),
        tokens_reduction_pct: calculate_reduction_pct(
            (control.input_tokens + control.output_tokens) as f64,
            (fmm.input_tokens + fmm.output_tokens) as f64,
        ),
        cost_reduction_pct: calculate_reduction_pct(control.total_cost_usd, fmm.total_cost_usd),
        duration_reduction_pct: calculate_reduction_pct(
            control.duration_ms as f64,
            fmm.duration_ms as f64,
        ),
    }
}

fn calculate_reduction_pct(control: f64, fmm: f64) -> f64 {
    if control == 0.0 {
        0.0
    } else {
        ((control - fmm) / control) * 100.0
    }
}

fn truncate(s: &str, max_len: usize) -> String {
    if s.chars().count() <= max_len {
        s.to_string()
    } else {
        let truncated: String = s.chars().take(max_len.saturating_sub(3)).collect();
        format!("{}...", truncated)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    #[test]
    fn test_reduction_calculation() {
        assert_eq!(calculate_reduction_pct(100.0, 50.0), 50.0);
        assert_eq!(calculate_reduction_pct(8.0, 1.0), 87.5);
        assert_eq!(calculate_reduction_pct(0.0, 10.0), 0.0);
    }

    #[test]
    fn test_empty_report_markdown_no_panic() {
        // Empty results should not panic on division by zero
        let report = ComparisonReport::new(
            "test-job".to_string(),
            "https://github.com/test/repo".to_string(),
            "abc123".to_string(),
            "main".to_string(),
            vec![], // Empty results
        );

        // Should not panic - just verify it runs without crashing
        let markdown = report.to_markdown();
        // The key test is that to_markdown() doesn't panic with empty results
        assert!(!markdown.is_empty());
        assert!(markdown.contains("Summary"));
    }

    fn create_test_run_result(task_id: &str, variant: &str, tool_calls: u32) -> RunResult {
        RunResult {
            task_id: task_id.to_string(),
            variant: variant.to_string(),
            tool_calls,
            tools_by_name: HashMap::new(),
            files_accessed: vec![],
            read_calls: tool_calls / 2,
            input_tokens: 1000,
            output_tokens: 500,
            cache_read_tokens: 0,
            total_cost_usd: 0.01,
            duration_ms: 1000,
            num_turns: 2,
            response: "test".to_string(),
            success: true,
            error: None,
        }
    }

    #[test]
    fn test_report_with_results() {
        use crate::tasks::{Task, TaskCategory};

        let task = Task {
            id: "test_task".to_string(),
            name: "Test Task".to_string(),
            prompt: "Test prompt".to_string(),
            category: TaskCategory::Exploration,
            expected_patterns: vec![],
            max_turns: 10,
            max_budget_usd: 1.0,
        };

        let control = create_test_run_result("test_task", "control", 10);
        let fmm = create_test_run_result("test_task", "fmm", 5);

        let report = ComparisonReport::new(
            "test-job".to_string(),
            "https://github.com/test/repo".to_string(),
            "abc123".to_string(),
            "main".to_string(),
            vec![(task, control, fmm)],
        );

        assert_eq!(report.summary.tasks_run, 1);
        assert_eq!(report.summary.fmm_wins, 1);
        assert_eq!(report.summary.control_wins, 0);
        assert_eq!(
            report.task_results[0].savings.tool_calls_reduction_pct,
            50.0
        );
    }
}
