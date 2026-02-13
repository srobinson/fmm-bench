//! Sandbox management for isolated comparison runs

use anyhow::{Context, Result};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

/// Sandbox for isolated repo comparison
pub struct Sandbox {
    /// Root directory for this sandbox
    pub root: PathBuf,
    /// Control variant directory (no FMM)
    pub control_dir: PathBuf,
    /// FMM variant directory (with manifest)
    pub fmm_dir: PathBuf,
    /// Whether to cleanup on drop
    cleanup_on_drop: bool,
}

impl Sandbox {
    /// Create a new sandbox for a job
    pub fn new(job_id: &str) -> Result<Self> {
        validate_job_id(job_id)?;
        let root = std::env::temp_dir().join(format!("fmm-compare-{}", job_id));
        fs::create_dir_all(&root).context("Failed to create sandbox root")?;

        let control_dir = root.join("control");
        let fmm_dir = root.join("fmm");

        Ok(Self {
            root,
            control_dir,
            fmm_dir,
            cleanup_on_drop: true,
        })
    }

    /// Clone a repository into the sandbox
    pub fn clone_repo(&self, url: &str, branch: Option<&str>) -> Result<()> {
        validate_repo_url(url)?;
        // Clone for control variant
        self.clone_to_dir(url, branch, &self.control_dir)?;

        // Clone for FMM variant (or copy)
        self.clone_to_dir(url, branch, &self.fmm_dir)?;

        Ok(())
    }

    fn clone_to_dir(&self, url: &str, branch: Option<&str>, dir: &Path) -> Result<()> {
        let mut cmd = Command::new("git");
        cmd.arg("clone")
            .arg("--depth")
            .arg("1")
            .arg("--single-branch");

        if let Some(b) = branch {
            cmd.arg("--branch").arg(b);
        }

        cmd.arg(url).arg(dir);

        let output = cmd.output().context("Failed to execute git clone")?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            anyhow::bail!("Git clone failed: {}", stderr);
        }

        Ok(())
    }

    /// Get the current commit SHA from a directory
    pub fn get_commit_sha(&self, dir: &Path) -> Result<String> {
        let output = Command::new("git")
            .arg("rev-parse")
            .arg("HEAD")
            .current_dir(dir)
            .output()
            .context("Failed to get commit SHA")?;

        if !output.status.success() {
            anyhow::bail!("Git rev-parse failed");
        }

        Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
    }

    /// Generate FMM sidecars for the FMM variant
    pub fn generate_fmm_manifest(&self) -> Result<()> {
        let fmm_binary = std::env::current_exe().context("Failed to get current executable")?;

        let output = Command::new(&fmm_binary)
            .arg("generate")
            .arg(".")
            .current_dir(&self.fmm_dir)
            .output()
            .context("Failed to run fmm generate")?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            eprintln!("Warning: fmm generate had issues: {}", stderr);
        }

        Ok(())
    }

    /// Install skill file and MCP config in the FMM variant workspace.
    /// This enables the Skill + MCP delivery mechanism (proven best by Exp15).
    pub fn setup_fmm_integration(&self) -> Result<()> {
        let fmm_binary = std::env::current_exe().context("Failed to get current executable")?;

        // Run `fmm init --all` which installs both skill and .mcp.json
        let output = Command::new(&fmm_binary)
            .arg("init")
            .arg("--all")
            .current_dir(&self.fmm_dir)
            .output()
            .context("Failed to run fmm init")?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            anyhow::bail!("fmm init --all failed: {}", stderr);
        }

        Ok(())
    }

    /// Disable cleanup on drop (for debugging/testing)
    #[cfg(test)]
    pub fn keep_on_drop(&mut self) {
        self.cleanup_on_drop = false;
    }

    /// Manually cleanup the sandbox
    pub fn cleanup(&self) {
        if let Err(e) = fs::remove_dir_all(&self.root) {
            eprintln!("Warning: Failed to cleanup sandbox: {}", e);
        }
    }
}

impl Drop for Sandbox {
    fn drop(&mut self) {
        if self.cleanup_on_drop {
            self.cleanup();
        }
    }
}

/// Validate job_id contains only safe path characters
fn validate_job_id(job_id: &str) -> Result<()> {
    if job_id.is_empty() {
        anyhow::bail!("Job ID must not be empty");
    }
    if !job_id
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
    {
        anyhow::bail!(
            "Invalid job ID '{}': only alphanumeric, hyphens, and underscores allowed",
            job_id
        );
    }
    Ok(())
}

/// Validate repository URL is a safe HTTPS git URL
fn validate_repo_url(url: &str) -> Result<()> {
    if !url.starts_with("https://") {
        anyhow::bail!("Repository URL must use HTTPS: {}", url);
    }
    // Ensure it looks like a valid git host URL (github, gitlab, bitbucket, etc.)
    let host = url
        .strip_prefix("https://")
        .and_then(|s| s.split('/').next())
        .unwrap_or("");
    if host.is_empty() || !host.contains('.') {
        anyhow::bail!("Invalid repository host in URL: {}", url);
    }
    // Reject URLs with suspicious characters that could be used for injection
    if url.contains("..") || url.contains('\0') || url.contains(';') || url.contains('|') {
        anyhow::bail!("Repository URL contains invalid characters: {}", url);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sandbox_creation() {
        let sandbox = Sandbox::new("test-123").unwrap();
        assert!(sandbox.root.exists());

        // Cleanup
        sandbox.cleanup();
        assert!(!sandbox.root.exists());
    }

    #[test]
    fn test_sandbox_rejects_traversal_job_id() {
        assert!(Sandbox::new("../escape").is_err());
        assert!(Sandbox::new("foo/../bar").is_err());
        assert!(Sandbox::new("").is_err());
    }

    #[test]
    fn test_sandbox_accepts_valid_job_id() {
        let sandbox = Sandbox::new("cmp-abc123-0f3a").unwrap();
        assert!(sandbox.root.exists());
        sandbox.cleanup();
    }

    #[test]
    fn test_validate_repo_url_https_required() {
        assert!(validate_repo_url("http://github.com/foo/bar").is_err());
        assert!(validate_repo_url("git@github.com:foo/bar.git").is_err());
        assert!(validate_repo_url("ftp://github.com/foo/bar").is_err());
    }

    #[test]
    fn test_validate_repo_url_valid() {
        assert!(validate_repo_url("https://github.com/pmndrs/zustand").is_ok());
        assert!(validate_repo_url("https://gitlab.com/user/project").is_ok());
        assert!(validate_repo_url("https://bitbucket.org/team/repo").is_ok());
    }

    #[test]
    fn test_validate_repo_url_injection() {
        assert!(validate_repo_url("https://github.com/foo;rm -rf /").is_err());
        assert!(validate_repo_url("https://github.com/foo|cat /etc/passwd").is_err());
        assert!(validate_repo_url("https://github.com/../../../etc").is_err());
    }

    #[test]
    fn test_validate_repo_url_invalid_host() {
        assert!(validate_repo_url("https:///no-host").is_err());
        assert!(validate_repo_url("https://noperiod/repo").is_err());
    }

    #[test]
    fn test_validate_job_id_valid() {
        assert!(validate_job_id("cmp-abc-123").is_ok());
        assert!(validate_job_id("simple").is_ok());
        assert!(validate_job_id("with_underscore").is_ok());
    }

    #[test]
    fn test_validate_job_id_invalid() {
        assert!(validate_job_id("").is_err());
        assert!(validate_job_id("../escape").is_err());
        assert!(validate_job_id("has space").is_err());
        assert!(validate_job_id("has;semicolon").is_err());
    }

    #[test]
    fn test_sandbox_auto_cleanup_on_drop() {
        let root_path;
        {
            let sandbox = Sandbox::new("drop-test-001").unwrap();
            root_path = sandbox.root.clone();
            assert!(root_path.exists());
            // sandbox drops here
        }
        assert!(!root_path.exists());
    }

    #[test]
    fn test_sandbox_keep_on_drop() {
        let root_path;
        {
            let mut sandbox = Sandbox::new("keep-test-001").unwrap();
            sandbox.keep_on_drop();
            root_path = sandbox.root.clone();
            // sandbox drops here but should NOT cleanup
        }
        assert!(root_path.exists());
        // Manual cleanup
        let _ = fs::remove_dir_all(&root_path);
    }
}
