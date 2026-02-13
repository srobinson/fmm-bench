//! Benchmark task definitions

use serde::{Deserialize, Serialize};

/// A benchmark task to run against a repository
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Task {
    /// Unique identifier for the task
    pub id: String,
    /// Human-readable name
    pub name: String,
    /// The prompt to send to Claude
    pub prompt: String,
    /// Category of task (exploration, understanding, etc.)
    pub category: TaskCategory,
    /// Expected keywords or patterns in the response (for accuracy scoring)
    #[serde(default)]
    pub expected_patterns: Vec<String>,
    /// Maximum turns allowed
    #[serde(default = "default_max_turns")]
    pub max_turns: u32,
    /// Maximum budget for this task in USD
    #[serde(default = "default_max_budget")]
    pub max_budget_usd: f64,
}

fn default_max_turns() -> u32 {
    20
}

fn default_max_budget() -> f64 {
    2.0
}

/// Category of benchmark task
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TaskCategory {
    /// Find specific code elements
    Exploration,
    /// Understand architecture/patterns
    Understanding,
    /// Find dependencies/imports
    Dependencies,
    /// Locate specific exports
    Exports,
}

impl std::fmt::Display for TaskCategory {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            TaskCategory::Exploration => write!(f, "exploration"),
            TaskCategory::Understanding => write!(f, "understanding"),
            TaskCategory::Dependencies => write!(f, "dependencies"),
            TaskCategory::Exports => write!(f, "exports"),
        }
    }
}

/// A set of tasks for benchmarking
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskSet {
    /// Name of the task set
    pub name: String,
    /// Description
    pub description: String,
    /// Tasks in this set
    pub tasks: Vec<Task>,
}

impl TaskSet {
    /// Load the standard task set for generic repository benchmarking
    pub fn standard() -> Self {
        Self {
            name: "standard".to_string(),
            description: "Standard benchmark tasks for any codebase".to_string(),
            tasks: vec![
                Task {
                    id: "find_entry".to_string(),
                    name: "Find Entry Point".to_string(),
                    prompt: "What is the main entry point of this codebase? \
                             List the primary exported functions or classes."
                        .to_string(),
                    category: TaskCategory::Exploration,
                    expected_patterns: vec![
                        "export".to_string(),
                        "main".to_string(),
                        "index".to_string(),
                    ],
                    max_turns: 10,
                    max_budget_usd: 1.0,
                },
                Task {
                    id: "architecture".to_string(),
                    name: "Architecture Overview".to_string(),
                    prompt: "Describe the high-level architecture of this project. \
                             What are the main modules and how do they interact?"
                        .to_string(),
                    category: TaskCategory::Understanding,
                    expected_patterns: vec![
                        "module".to_string(),
                        "component".to_string(),
                        "import".to_string(),
                    ],
                    max_turns: 15,
                    max_budget_usd: 1.5,
                },
                Task {
                    id: "find_export".to_string(),
                    name: "Find Specific Export".to_string(),
                    prompt: "Find where the main public API is exported from. \
                             What functions or classes are available to consumers of this library?"
                        .to_string(),
                    category: TaskCategory::Exports,
                    expected_patterns: vec!["export".to_string(), "public".to_string()],
                    max_turns: 10,
                    max_budget_usd: 1.0,
                },
                Task {
                    id: "dependencies".to_string(),
                    name: "Dependency Analysis".to_string(),
                    prompt: "What are the key internal dependencies in this codebase? \
                             Which modules depend on which other modules?"
                        .to_string(),
                    category: TaskCategory::Dependencies,
                    expected_patterns: vec![
                        "import".to_string(),
                        "depend".to_string(),
                        "require".to_string(),
                    ],
                    max_turns: 15,
                    max_budget_usd: 1.5,
                },
                Task {
                    id: "file_count".to_string(),
                    name: "Codebase Stats".to_string(),
                    prompt: "How many source files are in this project? \
                             Provide a breakdown by file type or module."
                        .to_string(),
                    category: TaskCategory::Exploration,
                    expected_patterns: vec![
                        "file".to_string(),
                        "count".to_string(),
                        "total".to_string(),
                    ],
                    max_turns: 10,
                    max_budget_usd: 1.0,
                },
            ],
        }
    }

    /// Load a quick task set (fewer tasks, faster results)
    pub fn quick() -> Self {
        Self {
            name: "quick".to_string(),
            description: "Quick benchmark with fewer tasks".to_string(),
            tasks: vec![
                Task {
                    id: "find_entry".to_string(),
                    name: "Find Entry Point".to_string(),
                    prompt: "What is the main entry point of this codebase? \
                             List the primary exported functions or classes."
                        .to_string(),
                    category: TaskCategory::Exploration,
                    expected_patterns: vec![
                        "export".to_string(),
                        "main".to_string(),
                        "index".to_string(),
                    ],
                    max_turns: 10,
                    max_budget_usd: 1.0,
                },
                Task {
                    id: "architecture".to_string(),
                    name: "Architecture Overview".to_string(),
                    prompt: "Describe the high-level architecture of this project. \
                             What are the main modules and how do they interact?"
                        .to_string(),
                    category: TaskCategory::Understanding,
                    expected_patterns: vec![
                        "module".to_string(),
                        "component".to_string(),
                        "import".to_string(),
                    ],
                    max_turns: 15,
                    max_budget_usd: 1.5,
                },
            ],
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_standard_task_set() {
        let tasks = TaskSet::standard();
        assert_eq!(tasks.name, "standard");
        assert!(!tasks.tasks.is_empty());
    }

    #[test]
    fn test_quick_task_set() {
        let tasks = TaskSet::quick();
        assert_eq!(tasks.name, "quick");
        assert!(tasks.tasks.len() < TaskSet::standard().tasks.len());
    }
}
