//! Post-run evaluation and scoring.
//!
//! Runs automated checks in each sandbox after Claude exits:
//! diff stats, test suite detection/execution, build verification,
//! and assigns a letter grade.

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::Path;
use std::process::Command;
use std::time::Duration;

/// Timeout for test/build commands.
const CMD_TIMEOUT_SECS: u64 = 300; // 5 minutes

/// Post-run evaluation scores for one condition.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct EvalScores {
    pub has_commit: bool,
    pub tests_pass: bool,
    pub tests_existed: bool,
    pub build_passes: bool,
    pub files_touched: u32,
    pub diff_lines_added: u32,
    pub diff_lines_removed: u32,
    pub grade: String,
}

/// Evaluate the sandbox state after a run.
pub fn evaluate(sandbox_dir: &Path) -> Result<EvalScores> {
    let diff = capture_diff_stats(sandbox_dir)?;
    let has_commit = diff.files_changed > 0 || diff.lines_added > 0 || diff.lines_removed > 0;

    let runner = detect_test_runner(sandbox_dir);
    let (tests_existed, tests_pass) = if let Some(ref r) = runner {
        (true, run_command_ok(sandbox_dir, r))
    } else {
        (false, false)
    };

    let build_cmd = detect_build_command(sandbox_dir);
    let build_passes = if let Some(ref cmd) = build_cmd {
        run_command_ok(sandbox_dir, cmd)
    } else {
        // No build system detected — don't penalize
        true
    };

    let grade = compute_grade(has_commit, tests_existed, tests_pass, build_passes);

    Ok(EvalScores {
        has_commit,
        tests_pass,
        tests_existed,
        build_passes,
        files_touched: diff.files_changed,
        diff_lines_added: diff.lines_added,
        diff_lines_removed: diff.lines_removed,
        grade,
    })
}

// ── diff stats ──────────────────────────────────────────────────────────────

struct DiffStats {
    files_changed: u32,
    lines_added: u32,
    lines_removed: u32,
}

fn capture_diff_stats(dir: &Path) -> Result<DiffStats> {
    // Check how many commits exist (shallow clones may only have 1)
    let log_output = Command::new("git")
        .args(["rev-list", "--count", "HEAD"])
        .current_dir(dir)
        .output()
        .ok();

    let commit_count: u32 = log_output
        .as_ref()
        .and_then(|o| String::from_utf8_lossy(&o.stdout).trim().parse().ok())
        .unwrap_or(1);

    // If Claude committed (>1 commit), diff against parent to see committed changes
    let committed_diff = if commit_count >= 2 {
        let output = Command::new("git")
            .args(["diff", "HEAD~1", "--numstat"])
            .current_dir(dir)
            .output()
            .ok();
        output.and_then(|o| {
            let text = String::from_utf8_lossy(&o.stdout).to_string();
            if o.status.success() && !text.trim().is_empty() {
                Some(text)
            } else {
                None
            }
        })
    } else {
        None
    };

    // Fall back to uncommitted working-tree diff
    let diff_text = if let Some(text) = committed_diff {
        text
    } else {
        let output = Command::new("git")
            .args(["diff", "HEAD", "--numstat"])
            .current_dir(dir)
            .output()
            .context("git diff failed")?;
        String::from_utf8_lossy(&output.stdout).to_string()
    };

    parse_numstat(&diff_text)
}

fn parse_numstat(text: &str) -> Result<DiffStats> {
    let mut files_changed = 0u32;
    let mut lines_added = 0u32;
    let mut lines_removed = 0u32;

    for line in text.lines() {
        let parts: Vec<&str> = line.split('\t').collect();
        if parts.len() >= 3 {
            files_changed += 1;
            // Binary files show "-" instead of numbers
            if let Ok(added) = parts[0].parse::<u32>() {
                lines_added += added;
            }
            if let Ok(removed) = parts[1].parse::<u32>() {
                lines_removed += removed;
            }
        }
    }

    Ok(DiffStats {
        files_changed,
        lines_added,
        lines_removed,
    })
}

// ── test runner detection ───────────────────────────────────────────────────

/// Detect the test command for a repository.
pub fn detect_test_runner(dir: &Path) -> Option<Vec<String>> {
    // Cargo (Rust)
    if dir.join("Cargo.toml").exists() {
        return Some(vec!["cargo".into(), "test".into()]);
    }

    // Go
    if dir.join("go.mod").exists() {
        return Some(vec!["go".into(), "test".into(), "./...".into()]);
    }

    // Python
    if dir.join("pyproject.toml").exists() || dir.join("setup.py").exists() {
        return Some(vec!["python".into(), "-m".into(), "pytest".into()]);
    }

    // Node.js — check for package.json with test script
    let pkg_json = dir.join("package.json");
    if pkg_json.exists() {
        if let Ok(content) = std::fs::read_to_string(&pkg_json) {
            if let Ok(pkg) = serde_json::from_str::<serde_json::Value>(&content) {
                if let Some(test_script) = pkg.get("scripts").and_then(|s| s.get("test")) {
                    let test_str = test_script.as_str().unwrap_or("");
                    // Skip placeholder scripts like "echo \"Error: no test specified\""
                    if !test_str.is_empty()
                        && !test_str.contains("no test specified")
                        && !test_str.contains("exit 1")
                    {
                        let runner = if dir.join("pnpm-lock.yaml").exists() {
                            "pnpm"
                        } else if dir.join("yarn.lock").exists() {
                            "yarn"
                        } else {
                            "npm"
                        };
                        return Some(vec![runner.into(), "test".into()]);
                    }
                }
            }
        }
    }

    None
}

/// Detect the build command for a repository.
fn detect_build_command(dir: &Path) -> Option<Vec<String>> {
    if dir.join("Cargo.toml").exists() {
        return Some(vec!["cargo".into(), "build".into()]);
    }

    if dir.join("go.mod").exists() {
        return Some(vec!["go".into(), "build".into(), "./...".into()]);
    }

    // Node.js — check for build script
    let pkg_json = dir.join("package.json");
    if pkg_json.exists() {
        if let Ok(content) = std::fs::read_to_string(&pkg_json) {
            if let Ok(pkg) = serde_json::from_str::<serde_json::Value>(&content) {
                if pkg
                    .get("scripts")
                    .and_then(|s| s.get("build"))
                    .and_then(|v| v.as_str())
                    .is_some()
                {
                    let runner = if dir.join("pnpm-lock.yaml").exists() {
                        "pnpm"
                    } else if dir.join("yarn.lock").exists() {
                        "yarn"
                    } else {
                        "npm"
                    };
                    return Some(vec![runner.into(), "run".into(), "build".into()]);
                }
            }
        }
    }

    // Python — no universal build step
    None
}

// ── command execution ───────────────────────────────────────────────────────

fn run_command_ok(dir: &Path, cmd: &[String]) -> bool {
    if cmd.is_empty() {
        return false;
    }

    let Ok(mut child) = Command::new(&cmd[0])
        .args(&cmd[1..])
        .current_dir(dir)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
    else {
        return false;
    };

    let timeout = Duration::from_secs(CMD_TIMEOUT_SECS);
    let start = std::time::Instant::now();

    loop {
        match child.try_wait() {
            Ok(Some(status)) => return status.success(),
            Ok(None) => {
                if start.elapsed() > timeout {
                    let _ = child.kill();
                    return false;
                }
                std::thread::sleep(Duration::from_millis(250));
            }
            Err(_) => return false,
        }
    }
}

// ── grading ─────────────────────────────────────────────────────────────────

fn compute_grade(
    has_commit: bool,
    tests_existed: bool,
    tests_pass: bool,
    build_passes: bool,
) -> String {
    if !has_commit {
        return "F".to_string();
    }

    if !build_passes {
        return "D".to_string();
    }

    if tests_existed && tests_pass {
        return "A".to_string();
    }

    if tests_existed && !tests_pass {
        return "C".to_string();
    }

    // Build passes, no tests to validate
    "B".to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn grade_a_tests_pass() {
        assert_eq!(compute_grade(true, true, true, true), "A");
    }

    #[test]
    fn grade_b_no_tests() {
        assert_eq!(compute_grade(true, false, false, true), "B");
    }

    #[test]
    fn grade_c_tests_fail() {
        assert_eq!(compute_grade(true, true, false, true), "C");
    }

    #[test]
    fn grade_d_build_fails() {
        assert_eq!(compute_grade(true, true, true, false), "D");
    }

    #[test]
    fn grade_f_no_commit() {
        assert_eq!(compute_grade(false, true, true, true), "F");
    }

    #[test]
    fn parse_numstat_basic() {
        let input = "10\t3\tsrc/main.rs\n5\t0\tsrc/lib.rs\n";
        let stats = parse_numstat(input).unwrap();
        assert_eq!(stats.files_changed, 2);
        assert_eq!(stats.lines_added, 15);
        assert_eq!(stats.lines_removed, 3);
    }

    #[test]
    fn parse_numstat_empty() {
        let stats = parse_numstat("").unwrap();
        assert_eq!(stats.files_changed, 0);
        assert_eq!(stats.lines_added, 0);
        assert_eq!(stats.lines_removed, 0);
    }

    #[test]
    fn parse_numstat_binary() {
        let input = "-\t-\timage.png\n5\t2\tsrc/app.rs\n";
        let stats = parse_numstat(input).unwrap();
        assert_eq!(stats.files_changed, 2);
        assert_eq!(stats.lines_added, 5);
        assert_eq!(stats.lines_removed, 2);
    }

    #[test]
    fn detect_cargo_test_runner() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("Cargo.toml"), "[package]\nname = \"test\"").unwrap();
        let runner = detect_test_runner(dir.path());
        assert_eq!(runner, Some(vec!["cargo".to_string(), "test".to_string()]));
    }

    #[test]
    fn detect_go_test_runner() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("go.mod"), "module test").unwrap();
        let runner = detect_test_runner(dir.path());
        assert_eq!(
            runner,
            Some(vec![
                "go".to_string(),
                "test".to_string(),
                "./...".to_string()
            ])
        );
    }

    #[test]
    fn detect_python_test_runner() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("pyproject.toml"), "[project]").unwrap();
        let runner = detect_test_runner(dir.path());
        assert_eq!(
            runner,
            Some(vec![
                "python".to_string(),
                "-m".to_string(),
                "pytest".to_string()
            ])
        );
    }

    #[test]
    fn detect_npm_test_runner() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("package.json"),
            r#"{"scripts": {"test": "vitest run"}}"#,
        )
        .unwrap();
        let runner = detect_test_runner(dir.path());
        assert_eq!(runner, Some(vec!["npm".to_string(), "test".to_string()]));
    }

    #[test]
    fn detect_pnpm_test_runner() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("package.json"),
            r#"{"scripts": {"test": "jest"}}"#,
        )
        .unwrap();
        std::fs::write(dir.path().join("pnpm-lock.yaml"), "lockfileVersion: 9").unwrap();
        let runner = detect_test_runner(dir.path());
        assert_eq!(runner, Some(vec!["pnpm".to_string(), "test".to_string()]));
    }

    #[test]
    fn detect_yarn_test_runner() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("package.json"),
            r#"{"scripts": {"test": "jest"}}"#,
        )
        .unwrap();
        std::fs::write(dir.path().join("yarn.lock"), "# yarn lockfile v1").unwrap();
        let runner = detect_test_runner(dir.path());
        assert_eq!(runner, Some(vec!["yarn".to_string(), "test".to_string()]));
    }

    #[test]
    fn detect_npm_placeholder_skipped() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("package.json"),
            r#"{"scripts": {"test": "echo \"Error: no test specified\" && exit 1"}}"#,
        )
        .unwrap();
        let runner = detect_test_runner(dir.path());
        assert!(runner.is_none());
    }

    #[test]
    fn detect_no_test_runner() {
        let dir = tempfile::tempdir().unwrap();
        let runner = detect_test_runner(dir.path());
        assert!(runner.is_none());
    }

    #[test]
    fn evaluate_empty_dir() {
        let dir = tempfile::tempdir().unwrap();
        // Init a git repo so git commands work
        Command::new("git")
            .args(["init"])
            .current_dir(dir.path())
            .output()
            .unwrap();
        Command::new("git")
            .args(["commit", "--allow-empty", "-m", "init"])
            .current_dir(dir.path())
            .output()
            .unwrap();

        let scores = evaluate(dir.path()).unwrap();
        assert!(!scores.has_commit);
        assert_eq!(scores.files_touched, 0);
        assert_eq!(scores.grade, "F");
    }
}
