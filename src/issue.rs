//! GitHub issue fetching and prompt construction.
//!
//! Parses issue identifiers in multiple formats, fetches via `gh` CLI,
//! and constructs identical prompts for A/B comparison conditions.

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::process::Command;

/// A parsed GitHub issue reference.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IssueRef {
    pub owner: String,
    pub repo: String,
    pub number: u64,
}

impl IssueRef {
    /// Full `owner/repo` identifier.
    pub fn repo_slug(&self) -> String {
        format!("{}/{}", self.owner, self.repo)
    }

    /// The `owner/repo#N` short form.
    pub fn short_id(&self) -> String {
        format!("{}#{}", self.repo_slug(), self.number)
    }

    /// HTTPS clone URL for this repo.
    pub fn clone_url(&self) -> String {
        format!("https://github.com/{}/{}", self.owner, self.repo)
    }
}

impl std::fmt::Display for IssueRef {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}/{}#{}", self.owner, self.repo, self.number)
    }
}

/// Fetched issue data from GitHub.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GitHubIssue {
    pub issue_ref: IssueRef,
    pub title: String,
    pub body: String,
    pub state: String,
    pub labels: Vec<String>,
}

impl GitHubIssue {
    /// Build the benchmark prompt for this issue.
    ///
    /// Both conditions (control and fmm) receive the exact same prompt.
    pub fn to_prompt(&self) -> String {
        format!(
            r#"Here is a GitHub issue for this repository:

## {}

{}

---

Fix this issue. Make the minimal changes needed to resolve it.
Do not modify tests unless the issue specifically requires test changes.
When done, commit your changes with a descriptive message."#,
            self.title, self.body
        )
    }
}

/// Parse an issue identifier string into an IssueRef.
///
/// Supported formats:
/// - `owner/repo#123`
/// - `https://github.com/owner/repo/issues/123`
/// - `owner/repo/issues/123`
pub fn parse_issue_identifier(input: &str) -> Result<IssueRef> {
    let input = input.trim();

    // Format: https://github.com/owner/repo/issues/123
    if let Some(rest) = input
        .strip_prefix("https://github.com/")
        .or_else(|| input.strip_prefix("http://github.com/"))
    {
        return parse_path_with_issues(rest);
    }

    // Format: owner/repo#123
    if let Some((slug, num_str)) = input.split_once('#') {
        let number: u64 = num_str
            .parse()
            .with_context(|| format!("Invalid issue number: '{}'", num_str))?;
        let (owner, repo) = parse_owner_repo(slug)?;
        return Ok(IssueRef {
            owner,
            repo,
            number,
        });
    }

    // Format: owner/repo/issues/123
    if input.contains("/issues/") {
        return parse_path_with_issues(input);
    }

    anyhow::bail!(
        "Could not parse issue identifier: '{}'\n\
         Expected: owner/repo#123, https://github.com/owner/repo/issues/123, or owner/repo/issues/123",
        input
    )
}

/// Parse `owner/repo/issues/N` path format.
fn parse_path_with_issues(path: &str) -> Result<IssueRef> {
    let parts: Vec<&str> = path.split('/').collect();
    if parts.len() < 4 || parts[2] != "issues" {
        anyhow::bail!("Expected format: owner/repo/issues/N, got: '{}'", path);
    }

    let owner = validate_component(parts[0], "owner")?;
    let repo = validate_component(parts[1], "repo")?;
    let number: u64 = parts[3]
        .parse()
        .with_context(|| format!("Invalid issue number: '{}'", parts[3]))?;

    Ok(IssueRef {
        owner,
        repo,
        number,
    })
}

/// Parse `owner/repo` into (owner, repo).
fn parse_owner_repo(slug: &str) -> Result<(String, String)> {
    let parts: Vec<&str> = slug.split('/').collect();
    if parts.len() != 2 {
        anyhow::bail!("Expected owner/repo, got: '{}'", slug);
    }
    let owner = validate_component(parts[0], "owner")?;
    let repo = validate_component(parts[1], "repo")?;
    Ok((owner, repo))
}

/// Validate a GitHub owner or repo name component.
fn validate_component(s: &str, label: &str) -> Result<String> {
    if s.is_empty() {
        anyhow::bail!("GitHub {} must not be empty", label);
    }
    if !s
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.')
    {
        anyhow::bail!(
            "Invalid GitHub {}: '{}' (only alphanumeric, hyphens, underscores, and dots allowed)",
            label,
            s
        );
    }
    Ok(s.to_string())
}

/// Fetch a GitHub issue using the `gh` CLI.
pub fn fetch_issue(issue_ref: &IssueRef) -> Result<GitHubIssue> {
    let repo_arg = issue_ref.repo_slug();

    let output = Command::new("gh")
        .args([
            "issue",
            "view",
            &issue_ref.number.to_string(),
            "--repo",
            &repo_arg,
            "--json",
            "title,body,labels,state",
        ])
        .output()
        .context("Failed to execute `gh` CLI. Is it installed and authenticated?")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        if stderr.contains("not found") || stderr.contains("Could not resolve") {
            anyhow::bail!(
                "Issue {} not found. It may be private, deleted, or the repo doesn't exist.\n{}",
                issue_ref,
                stderr.trim()
            );
        }
        anyhow::bail!("Failed to fetch {}: {}", issue_ref, stderr.trim());
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let data: serde_json::Value =
        serde_json::from_str(&stdout).context("Failed to parse `gh` JSON output")?;

    let title = data["title"].as_str().unwrap_or("(no title)").to_string();
    let body = data["body"].as_str().unwrap_or("").to_string();
    let state = data["state"].as_str().unwrap_or("UNKNOWN").to_string();
    let labels = data["labels"]
        .as_array()
        .map(|arr| {
            arr.iter()
                .filter_map(|l| l["name"].as_str().map(String::from))
                .collect()
        })
        .unwrap_or_default();

    Ok(GitHubIssue {
        issue_ref: issue_ref.clone(),
        title,
        body,
        state,
        labels,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_owner_repo_hash_format() {
        let r = parse_issue_identifier("srobinson/fmm#42").unwrap();
        assert_eq!(r.owner, "srobinson");
        assert_eq!(r.repo, "fmm");
        assert_eq!(r.number, 42);
    }

    #[test]
    fn parse_https_url_format() {
        let r = parse_issue_identifier("https://github.com/srobinson/fmm/issues/42").unwrap();
        assert_eq!(r.owner, "srobinson");
        assert_eq!(r.repo, "fmm");
        assert_eq!(r.number, 42);
    }

    #[test]
    fn parse_path_format() {
        let r = parse_issue_identifier("srobinson/fmm/issues/42").unwrap();
        assert_eq!(r.owner, "srobinson");
        assert_eq!(r.repo, "fmm");
        assert_eq!(r.number, 42);
    }

    #[test]
    fn parse_with_whitespace() {
        let r = parse_issue_identifier("  srobinson/fmm#42  ").unwrap();
        assert_eq!(r.owner, "srobinson");
        assert_eq!(r.number, 42);
    }

    #[test]
    fn parse_dotted_repo_name() {
        let r = parse_issue_identifier("owner/repo.js#1").unwrap();
        assert_eq!(r.repo, "repo.js");
        assert_eq!(r.number, 1);
    }

    #[test]
    fn parse_invalid_no_number() {
        assert!(parse_issue_identifier("srobinson/fmm").is_err());
    }

    #[test]
    fn parse_invalid_bad_number() {
        assert!(parse_issue_identifier("srobinson/fmm#abc").is_err());
    }

    #[test]
    fn parse_invalid_empty() {
        assert!(parse_issue_identifier("").is_err());
    }

    #[test]
    fn parse_invalid_just_number() {
        assert!(parse_issue_identifier("42").is_err());
    }

    #[test]
    fn parse_invalid_bad_url() {
        assert!(parse_issue_identifier("https://github.com/only-owner").is_err());
    }

    #[test]
    fn issue_ref_display() {
        let r = IssueRef {
            owner: "srobinson".to_string(),
            repo: "fmm".to_string(),
            number: 42,
        };
        assert_eq!(r.to_string(), "srobinson/fmm#42");
        assert_eq!(r.short_id(), "srobinson/fmm#42");
        assert_eq!(r.clone_url(), "https://github.com/srobinson/fmm");
    }

    #[test]
    fn prompt_construction() {
        let issue = GitHubIssue {
            issue_ref: IssueRef {
                owner: "test".to_string(),
                repo: "repo".to_string(),
                number: 1,
            },
            title: "Fix the bug".to_string(),
            body: "The thing is broken.\n\nSteps to reproduce:\n1. Do X\n2. See Y".to_string(),
            state: "OPEN".to_string(),
            labels: vec!["bug".to_string()],
        };

        let prompt = issue.to_prompt();
        assert!(prompt.contains("## Fix the bug"));
        assert!(prompt.contains("The thing is broken."));
        assert!(prompt.contains("Fix this issue."));
        assert!(prompt.contains("commit your changes"));
    }

    #[test]
    fn prompt_identical_for_both_conditions() {
        let issue = GitHubIssue {
            issue_ref: IssueRef {
                owner: "a".to_string(),
                repo: "b".to_string(),
                number: 1,
            },
            title: "Title".to_string(),
            body: "Body".to_string(),
            state: "OPEN".to_string(),
            labels: vec![],
        };

        let p1 = issue.to_prompt();
        let p2 = issue.to_prompt();
        assert_eq!(p1, p2, "Prompt must be identical for both conditions");
    }

    #[test]
    fn validate_component_rejects_injection() {
        assert!(validate_component("foo;bar", "owner").is_err());
        assert!(validate_component("foo/bar", "owner").is_err());
        assert!(validate_component("", "owner").is_err());
        assert!(validate_component("foo bar", "owner").is_err());
    }

    #[test]
    fn validate_component_accepts_valid() {
        assert!(validate_component("srobinson", "owner").is_ok());
        assert!(validate_component("fmm", "repo").is_ok());
        assert!(validate_component("my-repo.js", "repo").is_ok());
        assert!(validate_component("user_name", "owner").is_ok());
    }
}
