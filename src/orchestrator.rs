//! Comparison orchestrator - coordinates all components

use anyhow::{Context, Result};
use colored::Colorize;
use std::fs;
use std::path::PathBuf;

use crate::cache::{CacheKey, CacheManager};
use crate::issue::GitHubIssue;
use crate::report::{ComparisonReport, ReportFormat};
use crate::runner::{ClaudeRunner, RunResult};
use crate::sandbox::Sandbox;
use crate::tasks::{Task, TaskCategory, TaskSet};

/// Options for comparison run
#[derive(Debug, Clone)]
pub struct CompareOptions {
    /// Branch to compare (default: main)
    pub branch: Option<String>,
    /// Path within repo to analyze (default: src/)
    pub src_path: Option<String>,
    /// Task set to use (standard, quick, or custom path)
    pub task_set: String,
    /// Number of runs per task (for averaging)
    pub runs: u32,
    /// Output directory for results
    pub output: Option<PathBuf>,
    /// Output format
    pub format: ReportFormat,
    /// Maximum budget in USD
    pub max_budget: f64,
    /// Use cached results when available
    pub use_cache: bool,
    /// Quick mode (fewer tasks)
    pub quick: bool,
    /// Model to use
    pub model: String,
}

impl Default for CompareOptions {
    fn default() -> Self {
        Self {
            branch: None,
            src_path: None,
            task_set: "standard".to_string(),
            runs: 1,
            output: None,
            format: ReportFormat::Both,
            max_budget: 10.0,
            use_cache: true,
            quick: false,
            model: "sonnet".to_string(),
        }
    }
}

/// Orchestrator for comparison runs
pub struct Orchestrator {
    options: CompareOptions,
    cache: CacheManager,
    /// Runner for control variant (fully isolated — no skills, no MCP)
    control_runner: ClaudeRunner,
    /// Runner for FMM variant (local settings — picks up skill + MCP from workspace)
    fmm_runner: ClaudeRunner,
    total_cost: f64,
}

impl Orchestrator {
    /// Create a new orchestrator
    pub fn new(options: CompareOptions) -> Result<Self> {
        let cache = CacheManager::new(None)?;
        let mut control_runner = ClaudeRunner::new();
        let mut fmm_runner = ClaudeRunner::with_local_settings();

        control_runner.set_model(&options.model);
        fmm_runner.set_model(&options.model);

        Ok(Self {
            options,
            cache,
            control_runner,
            fmm_runner,
            total_cost: 0.0,
        })
    }

    /// Run comparison on a repository
    pub fn run(&mut self, url: &str) -> Result<ComparisonReport> {
        let job_id = generate_job_id();

        println!("{} Job ID: {}", "📋".yellow(), job_id.cyan());

        // Step 1: Create sandbox and clone repo
        println!("{} Setting up sandbox...", "🔧".yellow());
        let sandbox = Sandbox::new(&job_id)?;
        sandbox.clone_repo(url, self.options.branch.as_deref())?;

        let commit_sha = sandbox.get_commit_sha(&sandbox.control_dir)?;
        let sha_display = if commit_sha.len() >= 8 {
            &commit_sha[..8]
        } else {
            &commit_sha
        };
        println!(
            "  {} Cloned at commit {}",
            "✓".green(),
            sha_display.dimmed()
        );

        // Step 2: Generate FMM sidecars + install skill + MCP for FMM variant
        println!("{} Setting up FMM variant...", "🔧".yellow());
        sandbox.generate_fmm_sidecars()?;

        let sidecar_count = walkdir::WalkDir::new(&sandbox.fmm_dir)
            .into_iter()
            .filter_map(|e| e.ok())
            .filter(|e| e.path().extension().and_then(|ext| ext.to_str()) == Some("fmm"))
            .count();
        if sidecar_count > 0 {
            println!(
                "  {} {} sidecar files generated",
                "✓".green(),
                sidecar_count
            );
        } else {
            println!(
                "  {} No sidecars generated (unsupported language?)",
                "!".yellow()
            );
        }

        // Install skill file + .mcp.json so Claude picks them up via --setting-sources local
        sandbox.setup_fmm_integration()?;
        println!(
            "  {} Installed skill + MCP config (Exp15-proven delivery)",
            "✓".green()
        );

        // Step 3: Load tasks
        let task_set = if self.options.quick {
            TaskSet::quick()
        } else {
            match self.options.task_set.as_str() {
                "standard" => TaskSet::standard(),
                "quick" => TaskSet::quick(),
                path => self.load_custom_tasks(path)?,
            }
        };

        println!(
            "{} Running {} tasks...",
            "🚀".yellow(),
            task_set.tasks.len()
        );

        // Step 4: Run tasks
        let mut results: Vec<(Task, RunResult, RunResult)> = vec![];

        for (i, task) in task_set.tasks.iter().enumerate() {
            println!(
                "\n{} Task {}/{}: {}",
                "▶".cyan(),
                i + 1,
                task_set.tasks.len(),
                task.name.white().bold()
            );

            // Check budget
            if self.total_cost >= self.options.max_budget {
                println!(
                    "{} Budget limit reached (${:.2} / ${:.2})",
                    "⚠".yellow(),
                    self.total_cost,
                    self.options.max_budget
                );
                break;
            }

            // Run control variant
            let control_result =
                self.run_task_with_cache(task, &sandbox.control_dir, "control", url, &commit_sha)?;

            // Run FMM variant
            let fmm_context = self.build_fmm_context(&sandbox.fmm_dir)?;
            let fmm_result = self.run_task_with_fmm(
                task,
                &sandbox.fmm_dir,
                "fmm",
                url,
                &commit_sha,
                &fmm_context,
            )?;

            // Update cost tracking
            self.total_cost += control_result.total_cost_usd + fmm_result.total_cost_usd;

            // Report progress
            let reduction = if control_result.tool_calls > 0 {
                ((control_result.tool_calls as f64 - fmm_result.tool_calls as f64)
                    / control_result.tool_calls as f64)
                    * 100.0
            } else {
                0.0
            };

            println!(
                "  Control: {} tools | FMM: {} tools | Reduction: {:.1}%",
                control_result.tool_calls, fmm_result.tool_calls, reduction
            );

            results.push((task.clone(), control_result, fmm_result));
        }

        // Step 5: Generate report
        println!("\n{} Generating report...", "📊".yellow());
        let branch = self
            .options
            .branch
            .clone()
            .unwrap_or_else(|| "main".to_string());
        let report = ComparisonReport::new(job_id, url.to_string(), commit_sha, branch, results);

        // Save report
        if let Some(ref output_dir) = self.options.output {
            let saved = report.save(output_dir, self.options.format)?;
            for path in saved {
                println!("  {} Saved: {}", "✓".green(), path.dimmed());
            }
        }

        // Also save to cache
        let report_path = self.cache.save_report(&report)?;
        println!(
            "  {} Cached: {}",
            "✓".green(),
            report_path.display().to_string().dimmed()
        );

        println!("\n{} Total cost: ${:.4}", "💰".yellow(), self.total_cost);

        Ok(report)
    }

    /// Run an issue-driven A/B comparison.
    ///
    /// Clones the repo, sets up control + fmm sandboxes, runs the issue prompt
    /// against both, and compares results.
    pub fn run_issue(&mut self, issue: &GitHubIssue) -> Result<ComparisonReport> {
        let job_id = generate_job_id();
        let url = &issue.issue_ref.clone_url();
        let issue_label = issue.issue_ref.short_id();

        println!(
            "{} Issue: {} — {}",
            ">>".yellow(),
            issue_label.cyan().bold(),
            issue.title.white()
        );
        println!("{} Job ID: {}", ">>".yellow(), job_id.cyan());

        // Step 1: Create sandbox and clone repo
        println!("{} Setting up sandbox...", ">>".yellow());
        let sandbox = Sandbox::new(&job_id)?;
        sandbox.clone_repo(url, self.options.branch.as_deref())?;

        let commit_sha = sandbox.get_commit_sha(&sandbox.control_dir)?;
        let sha_short = &commit_sha[..commit_sha.len().min(8)];
        println!("  {} Cloned at commit {}", "+".green(), sha_short.dimmed());

        // Step 2: Generate FMM sidecars + init for FMM variant
        println!("{} Setting up FMM variant...", ">>".yellow());
        sandbox.generate_fmm_sidecars()?;

        let sidecar_count = walkdir::WalkDir::new(&sandbox.fmm_dir)
            .into_iter()
            .filter_map(|e| e.ok())
            .filter(|e| e.path().extension().and_then(|ext| ext.to_str()) == Some("fmm"))
            .count();
        if sidecar_count > 0 {
            println!(
                "  {} {} sidecar files generated",
                "+".green(),
                sidecar_count
            );
        } else {
            println!(
                "  {} No sidecars generated (unsupported language?)",
                "!".yellow()
            );
        }

        sandbox.setup_fmm_integration()?;
        println!("  {} Installed CLAUDE.md + MCP config", "+".green());

        // Step 3: Build task from issue prompt
        let task = Task {
            id: format!("issue-{}", issue.issue_ref.number),
            name: issue.title.clone(),
            prompt: issue.to_prompt(),
            category: TaskCategory::Exploration,
            expected_patterns: vec![],
            max_turns: 50,
            max_budget_usd: self.options.max_budget,
        };

        // Step 4: Run N times
        let mut all_results: Vec<(Task, RunResult, RunResult)> = vec![];

        for run_idx in 0..self.options.runs {
            if self.options.runs > 1 {
                println!(
                    "\n{} Run {}/{}",
                    ">>".yellow(),
                    run_idx + 1,
                    self.options.runs
                );
            }

            // Check budget
            if self.total_cost >= self.options.max_budget * 2.0 * self.options.runs as f64 {
                println!(
                    "{} Budget limit reached (${:.2})",
                    "!".yellow(),
                    self.total_cost
                );
                break;
            }

            // Run control
            let control_result =
                self.run_task_with_cache(&task, &sandbox.control_dir, "control", url, &commit_sha)?;

            // Run FMM
            let fmm_context = self.build_fmm_context(&sandbox.fmm_dir)?;
            let fmm_result = self.run_task_with_fmm(
                &task,
                &sandbox.fmm_dir,
                "fmm",
                url,
                &commit_sha,
                &fmm_context,
            )?;

            self.total_cost += control_result.total_cost_usd + fmm_result.total_cost_usd;

            let reduction = if control_result.tool_calls > 0 {
                ((control_result.tool_calls as f64 - fmm_result.tool_calls as f64)
                    / control_result.tool_calls as f64)
                    * 100.0
            } else {
                0.0
            };

            println!(
                "  Control: {} tools, ${:.4} | FMM: {} tools, ${:.4} | Reduction: {:.1}%",
                control_result.tool_calls,
                control_result.total_cost_usd,
                fmm_result.tool_calls,
                fmm_result.total_cost_usd,
                reduction
            );

            all_results.push((task.clone(), control_result, fmm_result));

            // Reset sandbox git state between runs so each starts fresh
            if run_idx + 1 < self.options.runs {
                sandbox.reset_git_state()?;
            }
        }

        // Step 5: Generate report
        println!("\n{} Generating report...", ">>".yellow());
        let branch = self
            .options
            .branch
            .clone()
            .unwrap_or_else(|| "main".to_string());
        let report =
            ComparisonReport::new(job_id, url.to_string(), commit_sha, branch, all_results);

        if let Some(ref output_dir) = self.options.output {
            let saved = report.save(output_dir, self.options.format)?;
            for path in saved {
                println!("  {} Saved: {}", "+".green(), path.dimmed());
            }
        }

        let report_path = self.cache.save_report(&report)?;
        println!(
            "  {} Cached: {}",
            "+".green(),
            report_path.display().to_string().dimmed()
        );

        println!("\n{} Total cost: ${:.4}", ">>".yellow(), self.total_cost);

        Ok(report)
    }

    fn run_task_with_cache(
        &mut self,
        task: &Task,
        working_dir: &std::path::Path,
        variant: &str,
        repo_url: &str,
        commit_sha: &str,
    ) -> Result<RunResult> {
        // Check cache
        if self.options.use_cache {
            let cache_key = CacheKey::new(repo_url, commit_sha, &task.id, variant);
            if let Some(cached) = self.cache.get(&cache_key) {
                println!("  {} {} (cached)", "●".dimmed(), variant.dimmed());
                return Ok(cached);
            }
        }

        // Run task (control runner: fully isolated, no skill/MCP)
        print!("  {} {}...", "●".cyan(), variant);
        let result = self
            .control_runner
            .run_task(task, working_dir, variant, None)?;

        // Cache result
        if self.options.use_cache && result.success {
            let cache_key = CacheKey::new(repo_url, commit_sha, &task.id, variant);
            self.cache.set(cache_key, result.clone())?;
        }

        println!(
            " {} ({} tools, ${:.4})",
            if result.success {
                "✓".green()
            } else {
                "✗".red()
            },
            result.tool_calls,
            result.total_cost_usd
        );

        Ok(result)
    }

    fn run_task_with_fmm(
        &mut self,
        task: &Task,
        working_dir: &std::path::Path,
        variant: &str,
        repo_url: &str,
        commit_sha: &str,
        fmm_context: &str,
    ) -> Result<RunResult> {
        // Check cache
        if self.options.use_cache {
            let cache_key = CacheKey::new(repo_url, commit_sha, &task.id, variant);
            if let Some(cached) = self.cache.get(&cache_key) {
                println!("  {} {} (cached)", "●".dimmed(), variant.dimmed());
                return Ok(cached);
            }
        }

        // Run task (FMM runner: local settings enabled — picks up skill + MCP)
        print!("  {} {}...", "●".cyan(), variant);
        let context = if fmm_context.is_empty() {
            None
        } else {
            Some(fmm_context)
        };
        let result = self
            .fmm_runner
            .run_task(task, working_dir, variant, context)?;

        // Cache result
        if self.options.use_cache && result.success {
            let cache_key = CacheKey::new(repo_url, commit_sha, &task.id, variant);
            self.cache.set(cache_key, result.clone())?;
        }

        println!(
            " {} ({} tools, ${:.4})",
            if result.success {
                "✓".green()
            } else {
                "✗".red()
            },
            result.tool_calls,
            result.total_cost_usd
        );

        Ok(result)
    }

    fn build_fmm_context(&self, fmm_dir: &std::path::Path) -> Result<String> {
        // Check if sidecars exist
        let has_sidecars = walkdir::WalkDir::new(fmm_dir)
            .into_iter()
            .filter_map(|e| e.ok())
            .any(|e| e.path().extension().and_then(|ext| ext.to_str()) == Some("fmm"));

        if !has_sidecars {
            return Ok(String::new());
        }

        let context = r#"This repository has .fmm sidecar files — structured metadata companions for source files.

For every source file (e.g. foo.ts), there may be a foo.ts.fmm containing:
- exports: what the file defines
- imports: external packages used
- dependencies: local files it imports
- loc: file size

Use sidecars to navigate: Grep "exports:.*SymbolName" **/*.fmm to find files.
Only open source files you need to edit."#;

        Ok(context.to_string())
    }

    fn load_custom_tasks(&self, path: &str) -> Result<TaskSet> {
        let content = fs::read_to_string(path)
            .with_context(|| format!("Failed to load custom tasks from {}", path))?;

        serde_json::from_str(&content)
            .with_context(|| format!("Failed to parse custom tasks from {}", path))
    }
}

fn generate_job_id() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};

    // Safe default: UNIX_EPOCH is always in the past; zero duration produces a valid job ID
    let duration = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();

    let timestamp = duration.as_secs();
    let nanos = duration.subsec_nanos();

    // Use nanoseconds for randomness within the same second
    let random: u16 = ((nanos / 1000) % 65536) as u16;

    format!("cmp-{:x}-{:04x}", timestamp, random)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    #[test]
    fn test_job_id_generation() {
        let id1 = generate_job_id();

        assert!(id1.starts_with("cmp-"));
        assert!(!id1.is_empty());
        assert!(id1.len() > 10);
    }

    #[test]
    fn test_job_id_format_safe_for_paths() {
        // Job IDs should only contain path-safe characters
        for _ in 0..10 {
            let id = generate_job_id();
            assert!(
                id.chars()
                    .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_'),
                "Job ID contains unsafe chars: {}",
                id
            );
        }
    }

    #[test]
    fn test_default_options() {
        let opts = CompareOptions::default();
        assert_eq!(opts.runs, 1);
        assert_eq!(opts.max_budget, 10.0);
        assert!(opts.use_cache);
        assert!(!opts.quick);
        assert_eq!(opts.task_set, "standard");
        assert_eq!(opts.model, "sonnet");
    }

    #[test]
    fn test_orchestrator_creation() {
        let opts = CompareOptions::default();
        let orchestrator = Orchestrator::new(opts).unwrap();
        assert!((orchestrator.total_cost - 0.0).abs() < f64::EPSILON);
    }

    #[test]
    fn test_budget_tracking_logic() {
        // Test that the budget check logic works correctly
        let opts = CompareOptions {
            max_budget: 0.05,
            ..Default::default()
        };

        let orchestrator = Orchestrator::new(opts).unwrap();

        // Initially under budget
        assert!(orchestrator.total_cost < orchestrator.options.max_budget);
    }

    // Integration test: report generation with real data structures
    #[test]
    fn test_report_generation_integration() {
        use crate::report::ComparisonReport;
        use crate::runner::RunResult;
        use crate::tasks::{Task, TaskCategory};

        let task = Task {
            id: "find_entry".to_string(),
            name: "Find Entry Point".to_string(),
            prompt: "What is the main entry point?".to_string(),
            category: TaskCategory::Exploration,
            expected_patterns: vec!["main".to_string()],
            max_turns: 10,
            max_budget_usd: 1.0,
        };

        let control = RunResult {
            task_id: "find_entry".to_string(),
            variant: "control".to_string(),
            tool_calls: 8,
            tools_by_name: HashMap::from([
                ("Read".to_string(), 5),
                ("Glob".to_string(), 2),
                ("Grep".to_string(), 1),
            ]),
            files_accessed: vec!["src/main.rs".to_string(), "src/lib.rs".to_string()],
            read_calls: 5,
            input_tokens: 5000,
            output_tokens: 1200,
            cache_read_tokens: 0,
            total_cost_usd: 0.02,
            duration_ms: 15000,
            num_turns: 4,
            response: "The main entry point is src/main.rs".to_string(),
            success: true,
            error: None,
            tool_details: HashMap::new(),
            navigation: Default::default(),
            fmm_usage: Default::default(),
        };

        let fmm = RunResult {
            task_id: "find_entry".to_string(),
            variant: "fmm".to_string(),
            tool_calls: 1,
            tools_by_name: HashMap::from([("Read".to_string(), 1)]),
            files_accessed: vec!["src/main.rs".to_string()],
            read_calls: 1,
            input_tokens: 2000,
            output_tokens: 800,
            cache_read_tokens: 500,
            total_cost_usd: 0.005,
            duration_ms: 5000,
            num_turns: 1,
            response: "The main entry point is src/main.rs".to_string(),
            success: true,
            error: None,
            tool_details: HashMap::new(),
            navigation: Default::default(),
            fmm_usage: Default::default(),
        };

        let report = ComparisonReport::new(
            "integration-test".to_string(),
            "https://github.com/test/repo".to_string(),
            "abc123def456".to_string(),
            "main".to_string(),
            vec![(task, control, fmm)],
        );

        assert_eq!(report.summary.tasks_run, 1);
        assert_eq!(report.summary.fmm_wins, 1);
        assert_eq!(report.summary.control_wins, 0);
        assert_eq!(report.summary.control_totals.total_tool_calls, 8);
        assert_eq!(report.summary.fmm_totals.total_tool_calls, 1);

        // Verify savings
        let savings = &report.task_results[0].savings;
        assert!((savings.tool_calls_reduction_pct - 87.5).abs() < 0.1);
        assert!((savings.read_calls_reduction_pct - 80.0).abs() < 0.1);

        // Verify markdown generation doesn't panic
        let md = report.to_markdown();
        assert!(md.contains("integration-test"));
        assert!(md.contains("87.5%") || md.contains("87.50%"));

        // Verify JSON serialization round-trip
        let json = serde_json::to_string(&report).unwrap();
        let deserialized: ComparisonReport = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.job_id, "integration-test");
        assert_eq!(deserialized.summary.fmm_wins, 1);
    }

    // Integration test: custom task loading
    #[test]
    fn test_custom_task_loading() {
        let temp = tempfile::tempdir().unwrap();
        let task_file = temp.path().join("custom_tasks.json");

        let tasks_json = serde_json::json!({
            "name": "custom",
            "description": "Custom test tasks",
            "tasks": [
                {
                    "id": "custom_task",
                    "name": "Custom Task",
                    "prompt": "Test prompt",
                    "category": "exploration",
                    "expected_patterns": ["test"],
                    "max_turns": 5,
                    "max_budget_usd": 0.5
                }
            ]
        });

        std::fs::write(&task_file, tasks_json.to_string()).unwrap();

        let orchestrator = Orchestrator::new(CompareOptions::default()).unwrap();
        let loaded = orchestrator
            .load_custom_tasks(task_file.to_str().unwrap())
            .unwrap();

        assert_eq!(loaded.name, "custom");
        assert_eq!(loaded.tasks.len(), 1);
        assert_eq!(loaded.tasks[0].id, "custom_task");
    }

    #[test]
    fn test_custom_task_loading_invalid_file() {
        let orchestrator = Orchestrator::new(CompareOptions::default()).unwrap();
        assert!(orchestrator
            .load_custom_tasks("/nonexistent/path.json")
            .is_err());
    }

    #[test]
    fn test_custom_task_loading_invalid_json() {
        let temp = tempfile::tempdir().unwrap();
        let task_file = temp.path().join("bad.json");
        std::fs::write(&task_file, "not valid json").unwrap();

        let orchestrator = Orchestrator::new(CompareOptions::default()).unwrap();
        assert!(orchestrator
            .load_custom_tasks(task_file.to_str().unwrap())
            .is_err());
    }
}
