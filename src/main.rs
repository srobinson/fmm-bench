use anyhow::Result;
use clap::{Parser, ValueEnum};
use colored::Colorize;
use std::path::PathBuf;

fn main() -> Result<()> {
    let cli = Cli::parse();

    let report_format = match cli.format {
        OutputFormat::Json => fmm_bench::ReportFormat::Json,
        OutputFormat::Markdown => fmm_bench::ReportFormat::Markdown,
        OutputFormat::Both => fmm_bench::ReportFormat::Both,
    };

    let options = fmm_bench::CompareOptions {
        branch: cli.branch,
        src_path: cli.src_path,
        task_set: cli.tasks,
        runs: cli.runs,
        output: cli.output,
        format: report_format,
        max_budget: cli.max_budget,
        use_cache: !cli.no_cache,
        quick: cli.quick,
        model: cli.model,
    };

    println!(
        "{} Starting comparison for {}",
        ">>".yellow(),
        cli.url.cyan().bold()
    );

    let mut orchestrator = fmm_bench::Orchestrator::new(options)?;
    let report = orchestrator.run(&cli.url)?;

    println!("\n{}", "=".repeat(60).dimmed());
    println!("{}", "COMPARISON RESULTS".green().bold());
    println!("{}", "=".repeat(60).dimmed());

    report.print_summary();

    Ok(())
}

#[derive(Parser)]
#[command(
    name = "fmm-bench",
    about = "Benchmark FMM vs control on a GitHub repository",
    version,
)]
struct Cli {
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

#[derive(Debug, Clone, Copy, ValueEnum)]
enum OutputFormat {
    Json,
    Markdown,
    Both,
}
