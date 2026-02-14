use anyhow::Result;
use clap::{Parser, Subcommand, ValueEnum};
use colored::Colorize;
use std::path::PathBuf;

fn main() -> Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Commands::Run(args) => cmd_run(args),
        Commands::Compare(args) => cmd_compare(args),
        Commands::Batch(args) => cmd_batch(args),
        Commands::Validate(args) => cmd_validate(args),
    }
}

/// Run an issue-driven A/B comparison.
fn cmd_run(args: RunArgs) -> Result<()> {
    let issue_ref = fmm_bench::issue::parse_issue_identifier(&args.issue)?;

    println!(
        "{} Fetching {}...",
        ">>".yellow(),
        issue_ref.to_string().cyan().bold()
    );

    let issue = fmm_bench::issue::fetch_issue(&issue_ref)?;

    println!(
        "{} {} [{}]",
        ">>".yellow(),
        issue.title.white().bold(),
        issue.state.dimmed()
    );

    let options = fmm_bench::CompareOptions {
        branch: args.branch,
        src_path: None,
        task_set: "standard".to_string(),
        runs: args.runs,
        output: args.output,
        format: to_report_format(args.format),
        max_budget: args.budget,
        use_cache: !args.no_cache,
        quick: false,
        model: args.model,
    };

    let mut orchestrator = fmm_bench::Orchestrator::new(options)?;
    let report = orchestrator.run_issue(&issue)?;

    println!("\n{}", "=".repeat(60).dimmed());
    println!("{}", "COMPARISON RESULTS".green().bold());
    println!("{}", "=".repeat(60).dimmed());

    report.print_summary();

    Ok(())
}

/// Run task-based comparison on a repository (original mode).
fn cmd_compare(args: CompareArgs) -> Result<()> {
    let options = fmm_bench::CompareOptions {
        branch: args.branch,
        src_path: args.src_path,
        task_set: args.tasks,
        runs: args.runs,
        output: args.output,
        format: to_report_format(args.format),
        max_budget: args.max_budget,
        use_cache: !args.no_cache,
        quick: args.quick,
        model: args.model,
    };

    println!(
        "{} Starting comparison for {}",
        ">>".yellow(),
        args.url.cyan().bold()
    );

    let mut orchestrator = fmm_bench::Orchestrator::new(options)?;
    let report = orchestrator.run(&args.url)?;

    println!("\n{}", "=".repeat(60).dimmed());
    println!("{}", "COMPARISON RESULTS".green().bold());
    println!("{}", "=".repeat(60).dimmed());

    report.print_summary();

    Ok(())
}

/// Run batch A/B comparisons across a corpus.
fn cmd_batch(args: BatchArgs) -> Result<()> {
    let corpus = fmm_bench::batch::load_corpus(&args.corpus)?;

    println!(
        "{} Loaded {} issues from {}",
        ">>".yellow(),
        corpus.len(),
        args.corpus.display()
    );

    let opts = fmm_bench::batch::BatchOptions {
        budget: args.budget,
        runs: args.runs,
        filter: args.filter,
        resume: args.resume,
        output: args.output,
        model: args.model,
    };

    let aggregate = fmm_bench::batch::run_batch(&corpus, &opts)?;

    println!("\n{}", "=".repeat(60).dimmed());
    println!("{}", "AGGREGATE RESULTS".green().bold());
    println!("{}", "=".repeat(60).dimmed());

    println!(
        "  Issues: {}/{} completed",
        aggregate.issues_completed, aggregate.issues_total
    );
    println!("  Total cost: ${:.2}", aggregate.total_cost);

    let s = &aggregate.summary;
    if s.n > 0 {
        println!(
            "  Tool calls: {:.1} (ctrl) vs {:.1} (fmm) = {:.1}% reduction",
            s.tool_calls.control_mean, s.tool_calls.fmm_mean, s.tool_calls.delta_pct
        );
        println!(
            "  Cost: ${:.3} (ctrl) vs ${:.3} (fmm) = {:.1}% savings",
            s.cost.control_mean, s.cost.fmm_mean, s.cost.delta_pct
        );
    }

    Ok(())
}

/// Validate a corpus file.
fn cmd_validate(args: ValidateArgs) -> Result<()> {
    let corpus = fmm_bench::batch::load_corpus(&args.corpus)?;

    println!(
        "{} Validating {} corpus entries...\n",
        ">>".yellow(),
        corpus.len()
    );

    let results = fmm_bench::batch::validate_corpus(&corpus);

    let accessible = results.iter().filter(|r| r.issue_accessible).count();
    let failed = results.iter().filter(|r| !r.issue_accessible).count();

    println!(
        "\n{} {} accessible, {} failed out of {}",
        ">>".green().bold(),
        accessible,
        failed,
        results.len()
    );

    if failed > 0 {
        println!("\n{} Failed entries:", "!".red());
        for r in results.iter().filter(|r| !r.issue_accessible) {
            println!(
                "  - {}: {}",
                r.id,
                r.error.as_deref().unwrap_or("unknown error")
            );
        }
        anyhow::bail!("{} corpus entries failed validation", failed);
    }

    Ok(())
}

fn to_report_format(fmt: OutputFormat) -> fmm_bench::ReportFormat {
    match fmt {
        OutputFormat::Json => fmm_bench::ReportFormat::Json,
        OutputFormat::Markdown => fmm_bench::ReportFormat::Markdown,
        OutputFormat::Both => fmm_bench::ReportFormat::Both,
    }
}

#[derive(Parser)]
#[command(
    name = "fmm-bench",
    about = "A/B benchmark FMM-assisted vs unassisted Claude on GitHub issues",
    version
)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Run an issue-driven A/B comparison
    Run(RunArgs),
    /// Run task-based comparison on a repository (original mode)
    Compare(CompareArgs),
    /// Run batch A/B comparisons across a corpus of issues
    Batch(BatchArgs),
    /// Validate a corpus file (check all issues are accessible)
    Validate(ValidateArgs),
}

#[derive(Parser)]
struct RunArgs {
    /// GitHub issue: owner/repo#N, full URL, or owner/repo/issues/N
    issue: String,

    /// Branch to clone (default: repo default branch)
    #[arg(short, long)]
    branch: Option<String>,

    /// Model to use for Claude CLI
    #[arg(long, default_value = "sonnet")]
    model: String,

    /// Max spend per condition in USD
    #[arg(long, default_value = "5.0")]
    budget: f64,

    /// Number of runs for statistical significance
    #[arg(long, default_value = "1")]
    runs: u32,

    /// Output directory for results
    #[arg(short, long)]
    output: Option<PathBuf>,

    /// Output format
    #[arg(long, value_enum, default_value = "both")]
    format: OutputFormat,

    /// Disable result caching
    #[arg(long)]
    no_cache: bool,
}

#[derive(Parser)]
struct CompareArgs {
    /// GitHub repository URL
    url: String,

    #[arg(short, long)]
    branch: Option<String>,

    #[arg(long)]
    src_path: Option<String>,

    #[arg(long, default_value = "standard")]
    tasks: String,

    #[arg(long, default_value = "1")]
    runs: u32,

    #[arg(short, long)]
    output: Option<PathBuf>,

    #[arg(long, value_enum, default_value = "both")]
    format: OutputFormat,

    #[arg(long, default_value = "10.0")]
    max_budget: f64,

    #[arg(long)]
    no_cache: bool,

    #[arg(long)]
    quick: bool,

    #[arg(long, default_value = "sonnet")]
    model: String,
}

#[derive(Parser)]
struct BatchArgs {
    /// Path to corpus JSON file
    corpus: PathBuf,

    /// Maximum total budget in USD
    #[arg(long, default_value = "50.0")]
    budget: f64,

    /// Number of runs per issue (for statistical significance)
    #[arg(long, default_value = "1")]
    runs: u32,

    /// Filter by language (case-insensitive)
    #[arg(long)]
    filter: Option<String>,

    /// Skip issues with cached results
    #[arg(long)]
    resume: bool,

    /// Output directory for aggregate report
    #[arg(short, long)]
    output: Option<PathBuf>,

    /// Model to use
    #[arg(long, default_value = "sonnet")]
    model: String,
}

#[derive(Parser)]
struct ValidateArgs {
    /// Path to corpus JSON file
    corpus: PathBuf,
}

#[derive(Debug, Clone, Copy, ValueEnum)]
enum OutputFormat {
    Json,
    Markdown,
    Both,
}
