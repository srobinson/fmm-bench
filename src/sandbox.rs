//! Sandbox management for isolated comparison runs.
//!
//! Creates paired sandbox directories (control + fmm) with identical repo
//! checkouts. The fmm variant gets sidecars + CLAUDE.md + MCP config installed.

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
    /// FMM variant directory (with sidecars + CLAUDE.md + MCP)
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

    /// Clone a repository into the sandbox (both control and fmm dirs).
    pub fn clone_repo(&self, url: &str, branch: Option<&str>) -> Result<()> {
        validate_repo_url(url)?;
        self.clone_to_dir(url, branch, &self.control_dir)?;
        self.clone_to_dir(url, branch, &self.fmm_dir)?;
        Ok(())
    }

    /// Clone a repository at a specific commit SHA.
    ///
    /// Does a shallow clone then fetches the exact commit (needed for corpus
    /// pinning where issues are tied to a specific commit).
    pub fn clone_repo_at_commit(
        &self,
        url: &str,
        commit: &str,
        branch: Option<&str>,
    ) -> Result<()> {
        validate_repo_url(url)?;
        for dir in [&self.control_dir, &self.fmm_dir] {
            self.clone_to_dir(url, branch, dir)?;
            // Checkout specific commit
            let output = Command::new("git")
                .args(["checkout", commit])
                .current_dir(dir)
                .output()
                .context("Failed to checkout commit")?;
            if !output.status.success() {
                let stderr = String::from_utf8_lossy(&output.stderr);
                anyhow::bail!("git checkout {} failed: {}", commit, stderr.trim());
            }
        }
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

    /// Generate FMM sidecars for the FMM variant using the `fmm` binary.
    ///
    /// Uses `fmm generate` which smartly creates new, updates stale, and
    /// skips unchanged sidecars.
    pub fn generate_fmm_sidecars(&self) -> Result<()> {
        let fmm_path = find_fmm_binary()?;

        let output = Command::new(&fmm_path)
            .arg("generate")
            .current_dir(&self.fmm_dir)
            .output()
            .context("Failed to run `fmm generate`")?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            eprintln!("Warning: fmm generate had issues: {}", stderr.trim());
        }

        Ok(())
    }

    /// Install CLAUDE.md + .mcp.json in the FMM variant workspace.
    ///
    /// Runs `fmm init --all --no-generate` to install:
    /// - `.claude/CLAUDE.md` with fmm navigation instructions
    /// - `.mcp.json` with fmm MCP server configuration
    /// - `.claude/skills/fmm-navigate.md` skill file
    ///
    /// The --no-generate flag skips sidecar generation since we already did it.
    /// Exp14 proved LLMs don't discover .fmm organically — this init is critical.
    pub fn setup_fmm_integration(&self) -> Result<()> {
        let fmm_path = find_fmm_binary()?;

        let output = Command::new(&fmm_path)
            .args(["init", "--all", "--no-generate"])
            .current_dir(&self.fmm_dir)
            .output()
            .context("Failed to run `fmm init --all`")?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            anyhow::bail!("fmm init --all failed: {}", stderr.trim());
        }

        Ok(())
    }

    /// Reset git state in both sandbox dirs (between repeated runs).
    pub fn reset_git_state(&self) -> Result<()> {
        for dir in [&self.control_dir, &self.fmm_dir] {
            if dir.exists() {
                let output = Command::new("git")
                    .args(["checkout", "."])
                    .current_dir(dir)
                    .output()
                    .context("Failed to reset git state")?;
                if !output.status.success() {
                    let stderr = String::from_utf8_lossy(&output.stderr);
                    anyhow::bail!("git checkout . failed: {}", stderr);
                }
                let output = Command::new("git")
                    .args(["clean", "-fd"])
                    .current_dir(dir)
                    .output()
                    .context("Failed to clean untracked files")?;
                if !output.status.success() {
                    let stderr = String::from_utf8_lossy(&output.stderr);
                    anyhow::bail!("git clean -fd failed: {}", stderr);
                }
            }
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

/// Find the `fmm` binary in PATH or a well-known location.
fn find_fmm_binary() -> Result<PathBuf> {
    // Check FMM_BIN env var first (for testing / custom installs)
    if let Ok(path) = std::env::var("FMM_BIN") {
        let p = PathBuf::from(&path);
        if p.exists() {
            return Ok(p);
        }
        anyhow::bail!("FMM_BIN is set to '{}' but the file does not exist", path);
    }

    // Check if `fmm` is in PATH
    let output = Command::new("which")
        .arg("fmm")
        .output()
        .context("Failed to search for fmm in PATH")?;

    if output.status.success() {
        let path = String::from_utf8_lossy(&output.stdout).trim().to_string();
        if !path.is_empty() {
            return Ok(PathBuf::from(path));
        }
    }

    // Check common cargo install location
    if let Some(home) = dirs::home_dir() {
        let cargo_bin = home.join(".cargo/bin/fmm");
        if cargo_bin.exists() {
            return Ok(cargo_bin);
        }
    }

    anyhow::bail!(
        "Could not find `fmm` binary. Install it with `cargo install fmm` \
         or set FMM_BIN environment variable."
    )
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
    let host = url
        .strip_prefix("https://")
        .and_then(|s| s.split('/').next())
        .unwrap_or("");
    if host.is_empty() || !host.contains('.') {
        anyhow::bail!("Invalid repository host in URL: {}", url);
    }
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
        }
        assert!(root_path.exists());
        let _ = fs::remove_dir_all(&root_path);
    }

    #[test]
    fn test_find_fmm_binary_and_env_override() {
        // First: ensure fmm is findable with clean env
        // (remove FMM_BIN in case another test leaked it)
        std::env::remove_var("FMM_BIN");
        let result = find_fmm_binary();
        assert!(
            result.is_ok(),
            "fmm binary should be findable: {:?}",
            result.err()
        );

        // Second: FMM_BIN pointing to nonexistent path should error
        std::env::set_var("FMM_BIN", "/nonexistent/fmm");
        let result = find_fmm_binary();
        assert!(result.is_err());
        std::env::remove_var("FMM_BIN");
    }
}
