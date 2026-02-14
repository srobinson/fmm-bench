//! Batch runner: execute A/B comparisons across a corpus of GitHub issues.

use anyhow::{Context, Result};
use colored::Colorize;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

use crate::aggregate::AggregateReport;
use crate::issue::{self, GitHubIssue};
use crate::orchestrator::{CompareOptions, Orchestrator};
use crate::report::ComparisonReport;

/// A single entry in the corpus file.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CorpusEntry {
    /// Identifier: "owner/repo#N"
    pub id: String,
    /// Repository owner/name
    pub repo: String,
    /// Issue number
    pub issue: u32,
    /// Primary language
    pub language: String,
    /// Codebase size category
    #[serde(default = "default_size")]
    pub size: String,
    /// Issue type (bugfix, feature, refactor, etc.)
    #[serde(default = "default_type")]
    pub r#type: String,
    /// Whether the repo has a test suite
    #[serde(default)]
    pub has_tests: bool,
    /// Expected files to be touched (for validation)
    #[serde(default)]
    pub expected_files: Vec<String>,
    /// Complexity: simple, medium, complex
    #[serde(default = "default_complexity")]
    pub complexity: String,
    /// Estimated number of files to touch
    #[serde(default)]
    pub estimated_files: u32,
    /// Human-readable notes
    #[serde(default)]
    pub notes: String,
    /// Optional branch to clone
    #[serde(default)]
    pub branch: Option<String>,
    /// Optional commit to pin to
    #[serde(default)]
    pub commit: Option<String>,
}

fn default_size() -> String {
    "medium".to_string()
}

fn default_type() -> String {
    "bugfix".to_string()
}

fn default_complexity() -> String {
    "medium".to_string()
}

/// Options for a batch run.
#[derive(Debug, Clone)]
pub struct BatchOptions {
    /// Maximum total spend across all issues
    pub budget: f64,
    /// Number of runs per issue
    pub runs: u32,
    /// Filter by language (case-insensitive)
    pub filter: Option<String>,
    /// Skip issues with cached results
    pub resume: bool,
    /// Output directory
    pub output: Option<PathBuf>,
    /// Model to use
    pub model: String,
}

impl Default for BatchOptions {
    fn default() -> Self {
        Self {
            budget: 50.0,
            runs: 1,
            filter: None,
            resume: false,
            output: None,
            model: "sonnet".to_string(),
        }
    }
}

/// Load and validate a corpus file.
pub fn load_corpus(path: &Path) -> Result<Vec<CorpusEntry>> {
    let content = fs::read_to_string(path)
        .with_context(|| format!("Failed to read corpus: {}", path.display()))?;

    let entries: Vec<CorpusEntry> = serde_json::from_str(&content)
        .with_context(|| format!("Failed to parse corpus: {}", path.display()))?;

    if entries.is_empty() {
        anyhow::bail!("Corpus is empty: {}", path.display());
    }

    Ok(entries)
}

/// Run a batch of A/B comparisons across corpus issues.
pub fn run_batch(corpus: &[CorpusEntry], opts: &BatchOptions) -> Result<AggregateReport> {
    let filtered: Vec<&CorpusEntry> = if let Some(ref lang) = opts.filter {
        let lang_lower = lang.to_lowercase();
        corpus
            .iter()
            .filter(|e| e.language.to_lowercase() == lang_lower)
            .collect()
    } else {
        corpus.iter().collect()
    };

    println!(
        "{} Batch: {} issues ({})",
        ">>".yellow(),
        filtered.len(),
        if let Some(ref f) = opts.filter {
            format!("filtered: {}", f)
        } else {
            "all".to_string()
        }
    );

    let mut reports: Vec<(CorpusEntry, ComparisonReport)> = vec![];
    let mut total_cost = 0.0f64;

    for (i, entry) in filtered.iter().enumerate() {
        // Budget check
        if total_cost >= opts.budget {
            println!(
                "\n{} Budget limit reached (${:.2} / ${:.2}), stopping.",
                "!".yellow(),
                total_cost,
                opts.budget
            );
            break;
        }

        println!(
            "\n{} [{}/{}] {} ({})",
            ">>".cyan().bold(),
            i + 1,
            filtered.len(),
            entry.id.white().bold(),
            entry.language.dimmed()
        );

        // Fetch issue
        let issue_id = format!("{}#{}", entry.repo, entry.issue);
        let issue_ref = match issue::parse_issue_identifier(&issue_id) {
            Ok(r) => r,
            Err(e) => {
                eprintln!("  {} Skipping {}: {}", "!".red(), entry.id, e);
                continue;
            }
        };

        let issue = match issue::fetch_issue(&issue_ref) {
            Ok(i) => i,
            Err(e) => {
                eprintln!("  {} Failed to fetch {}: {}", "!".red(), entry.id, e);
                continue;
            }
        };

        // Run comparison
        let compare_opts = CompareOptions {
            branch: entry.branch.clone(),
            src_path: None,
            task_set: "standard".to_string(),
            runs: opts.runs,
            output: None, // Individual reports saved via cache
            format: crate::report::ReportFormat::Json,
            max_budget: (opts.budget - total_cost).min(10.0), // Per-issue cap
            use_cache: opts.resume,
            quick: false,
            model: opts.model.clone(),
        };

        match run_single_issue(&issue, compare_opts) {
            Ok(report) => {
                let cost: f64 = report
                    .task_results
                    .iter()
                    .map(|t| t.control.total_cost_usd + t.fmm.total_cost_usd)
                    .sum();
                total_cost += cost;
                reports.push(((*entry).clone(), report));
            }
            Err(e) => {
                eprintln!("  {} Error on {}: {}", "!".red(), entry.id, e);
            }
        }
    }

    println!(
        "\n{} Batch complete: {}/{} issues, ${:.2} total",
        ">>".green().bold(),
        reports.len(),
        filtered.len(),
        total_cost
    );

    // Generate aggregate report
    let aggregate = AggregateReport::from_reports(reports, &opts.model, opts.runs);

    // Save aggregate if output dir specified
    if let Some(ref output_dir) = opts.output {
        fs::create_dir_all(output_dir)?;

        let json_path = output_dir.join("aggregate.json");
        let json = serde_json::to_string_pretty(&aggregate)?;
        fs::write(&json_path, &json)?;
        println!("  {} {}", "+".green(), json_path.display());

        let md_path = output_dir.join("aggregate.md");
        fs::write(&md_path, aggregate.to_markdown())?;
        println!("  {} {}", "+".green(), md_path.display());
    }

    Ok(aggregate)
}

fn run_single_issue(issue: &GitHubIssue, opts: CompareOptions) -> Result<ComparisonReport> {
    let mut orchestrator = Orchestrator::new(opts)?;
    orchestrator.run_issue(issue)
}

/// Validation result for a single corpus entry.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ValidationResult {
    pub id: String,
    pub issue_accessible: bool,
    pub issue_title: Option<String>,
    pub error: Option<String>,
}

/// Validate all corpus entries: check that issues are fetchable via `gh`.
pub fn validate_corpus(corpus: &[CorpusEntry]) -> Vec<ValidationResult> {
    let mut results = vec![];

    for (i, entry) in corpus.iter().enumerate() {
        print!("  [{}/{}] {} ...", i + 1, corpus.len(), entry.id.white());

        let issue_id = format!("{}#{}", entry.repo, entry.issue);
        let result =
            match issue::parse_issue_identifier(&issue_id).and_then(|r| issue::fetch_issue(&r)) {
                Ok(gh_issue) => {
                    println!(" {} {}", "+".green(), gh_issue.title.dimmed());
                    ValidationResult {
                        id: entry.id.clone(),
                        issue_accessible: true,
                        issue_title: Some(gh_issue.title),
                        error: None,
                    }
                }
                Err(e) => {
                    println!(" {} {}", "!".red(), e);
                    ValidationResult {
                        id: entry.id.clone(),
                        issue_accessible: false,
                        issue_title: None,
                        error: Some(e.to_string()),
                    }
                }
            };

        results.push(result);
    }

    results
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn load_corpus_valid() {
        let dir = tempfile::tempdir().unwrap();
        let corpus_path = dir.path().join("corpus.json");
        let corpus = serde_json::json!([
            {
                "id": "owner/repo#1",
                "repo": "owner/repo",
                "issue": 1,
                "language": "rust"
            },
            {
                "id": "owner/repo#2",
                "repo": "owner/repo",
                "issue": 2,
                "language": "typescript",
                "size": "large",
                "type": "feature",
                "has_tests": true,
                "expected_files": ["src/index.ts"]
            }
        ]);
        fs::write(&corpus_path, corpus.to_string()).unwrap();

        let entries = load_corpus(&corpus_path).unwrap();
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].language, "rust");
        assert_eq!(entries[0].size, "medium"); // default
        assert_eq!(entries[1].size, "large");
        assert!(entries[1].has_tests);
    }

    #[test]
    fn load_corpus_empty_rejected() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("empty.json");
        fs::write(&path, "[]").unwrap();
        assert!(load_corpus(&path).is_err());
    }

    #[test]
    fn load_corpus_invalid_json() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("bad.json");
        fs::write(&path, "not json").unwrap();
        assert!(load_corpus(&path).is_err());
    }

    #[test]
    fn load_corpus_missing_file() {
        assert!(load_corpus(Path::new("/nonexistent/corpus.json")).is_err());
    }

    #[test]
    fn corpus_entry_defaults() {
        let json = r#"{"id": "a/b#1", "repo": "a/b", "issue": 1, "language": "go"}"#;
        let entry: CorpusEntry = serde_json::from_str(json).unwrap();
        assert_eq!(entry.size, "medium");
        assert_eq!(entry.r#type, "bugfix");
        assert!(!entry.has_tests);
        assert!(entry.expected_files.is_empty());
    }

    #[test]
    fn batch_options_defaults() {
        let opts = BatchOptions::default();
        assert_eq!(opts.budget, 50.0);
        assert_eq!(opts.runs, 1);
        assert!(opts.filter.is_none());
        assert!(!opts.resume);
    }
}
