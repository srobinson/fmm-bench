mod cache;
mod metrics;
mod orchestrator;
mod report;
mod runner;
pub mod sandbox;
mod tasks;

pub use orchestrator::{CompareOptions, Orchestrator};
pub use report::{ComparisonReport, ReportFormat};

use anyhow::Result;
use colored::Colorize;

pub fn compare(url: &str, options: CompareOptions) -> Result<()> {
    println!(
        "{} Starting comparison for {}",
        ">>".yellow(),
        url.cyan().bold()
    );

    let mut orchestrator = Orchestrator::new(options)?;
    let report = orchestrator.run(url)?;

    println!("\n{}", "=".repeat(60).dimmed());
    println!("{}", "COMPARISON RESULTS".green().bold());
    println!("{}", "=".repeat(60).dimmed());

    report.print_summary();

    Ok(())
}
