pub mod aggregate;
pub mod batch;
mod cache;
pub mod evaluator;
pub mod issue;
pub mod metrics;
pub mod orchestrator;
pub mod report;
mod runner;
pub mod sandbox;
mod tasks;

pub use orchestrator::{CompareOptions, Orchestrator};
pub use report::{ComparisonReport, ReportFormat};
pub use runner::RunResult;
