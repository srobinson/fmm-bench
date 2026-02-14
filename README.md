# fmm-bench

A/B benchmarking harness that compares Claude's performance **with and without [fmm](https://github.com/srobinson/fmm)** (Frontmatter Matters) on real GitHub issues. Measures tokens, cost, tool calls, wall time, and code quality — then grades results.

## Prerequisites

- [Rust](https://rustup.rs/) (1.70+)
- [Claude CLI](https://docs.anthropic.com/en/docs/claude-cli) (`claude` on PATH)
- [GitHub CLI](https://cli.github.com/) (`gh` on PATH, authenticated)
- [fmm](https://github.com/srobinson/fmm) (`fmm` on PATH)
- [just](https://github.com/casey/just) (optional, for dev commands)

## Install

```bash
cargo install --path .
```

## Usage

### Single issue

Run an A/B comparison on one GitHub issue:

```bash
fmm-bench run owner/repo#123
```

Accepts multiple formats:

```bash
fmm-bench run owner/repo#123
fmm-bench run https://github.com/owner/repo/issues/123
fmm-bench run owner/repo/issues/123
```

Options:

```
--model <MODEL>    Claude model to use (default: sonnet)
--budget <BUDGET>  Max spend per condition in USD (default: 5.0)
--runs <RUNS>      Runs per condition for statistical significance (default: 1)
-o, --output <DIR> Output directory for results
--format <FMT>     json, markdown, or both (default: both)
--no-cache         Disable result caching
```

### Batch run

Run the full corpus (or a filtered subset):

```bash
fmm-bench batch corpus.json
fmm-bench batch corpus.json --filter rust
fmm-bench batch corpus.json --runs 3 --budget 100 --resume
```

Options:

```
--budget <BUDGET>  Total budget cap in USD (default: 50.0)
--runs <RUNS>      Runs per issue (default: 1)
--filter <LANG>    Filter by language (case-insensitive)
--resume           Skip issues with cached results
--model <MODEL>    Claude model to use (default: sonnet)
-o, --output <DIR> Output directory for aggregate report
```

### Validate corpus

Check that all issues in a corpus file are accessible:

```bash
fmm-bench validate corpus.json
```

### Legacy compare mode

Task-based comparison on a repository (original mode, pre-issue-driven):

```bash
fmm-bench compare https://github.com/owner/repo
```

## Corpus format

The corpus is a JSON array of issue descriptors:

```json
[
  {
    "id": "pmndrs/zustand#2942",
    "repo": "pmndrs/zustand",
    "issue": 2942,
    "language": "typescript",
    "size": "medium",
    "type": "bugfix",
    "complexity": "simple",
    "has_tests": true,
    "estimated_files": 2,
    "notes": "TypeScript strict mode issue with store types"
  }
]
```

The included `corpus.json` contains 20 issues across 9 languages (TypeScript, JavaScript, Python, Rust, Go, Java, Ruby, C++, C#).

## How it works

1. **Clone** — clones the repo at the issue's point in time
2. **Control run** — Claude solves the issue with no fmm assistance
3. **Treatment run** — Claude solves the same issue with fmm sidecars, MCP tools, and CLAUDE.md navigation hints
4. **Metrics** — extracts tokens, cost, tool calls, wall time, and navigation efficiency from Claude's stream-json output
5. **Evaluate** — runs tests, checks build, computes diff stats, assigns A-F grade
6. **Report** — markdown + JSON report with side-by-side comparison; batch mode adds Welch's t-test for statistical significance

## Development

```bash
just check    # clippy + fmt check
just build    # cargo build
just test     # cargo test
just fmt      # auto-format
```

## License

MIT
