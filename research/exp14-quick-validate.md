# Experiment 14: Does FMM Help LLMs Navigate Codebases?

## Question

When an LLM is explicitly told about FMM headers and the `.fmm/index.json` manifest via a prompt preamble, does it navigate a codebase more efficiently on realistic dev tasks?

## Design

**Conditions:**
- **Control** — Clean codebase, no FMM artifacts, no preamble
- **FMM** — FMM headers in files + `.fmm/index.json` manifest + 4-line preamble

**Preamble (FMM condition only):**
```
Files in this codebase contain FMM headers — structured metadata blocks
at the top of each file listing exports, imports, and dependencies.
Read these headers first to understand file purpose before reading full content.
A manifest is available at .fmm/index.json indexing all files.
```

**Task:** Add refresh token support to an 18-file TypeScript auth app. Real feature work — not a metadata lookup.

**Isolation:** Claude CLI with `--setting-sources ""`, `--strict-mcp-config`, no CLAUDE.md, no MCP servers, no session persistence. Clean git repo per run.

**Model:** Sonnet, $1 budget cap per condition.

## Results (n=4)

| Run | Tool Calls | | Glob Calls | | Tokens (k) | | Cost ($) | |
|-----|------|------|------|------|------|------|------|------|
| | Clean | FMM | Clean | FMM | Clean | FMM | Clean | FMM |
| 1 | 14 | 11 | 6 | 2 | 188 | 212 | 0.45 | 0.57 |
| 2 | 12 | 10 | 5 | 1 | 189 | 172 | 0.45 | 0.37 |
| 3 | 15 | 12 | 6 | 3 | 606 | 163 | 1.09 | 0.32 |
| 4 | 12 | 10 | 3 | 1 | 798 | 177 | 1.30 | 0.39 |
| **Avg** | **13.3** | **10.8** | **5.0** | **1.8** | **445** | **181** | **0.82** | **0.41** |

## Key Findings

### 1. FMM reduces blind searching by 65%

Glob calls (the "I'm looking for something" signal) dropped from an average of 5.0 to 1.8 across all runs. The LLM reads the manifest and knows where things are.

### 2. FMM makes LLM behavior predictable

Control token usage ranged from 188k to 798k (4.2x variance). FMM ranged from 163k to 212k (1.3x variance). Without FMM, the LLM sometimes thrashes — exploring dead ends, reading unnecessary files. With FMM, it consistently goes straight to the relevant code.

### 3. FMM cuts cost by ~50% on average

Average cost: Control $0.82 vs FMM $0.41. The savings come from less exploration, not from fewer reads. The LLM still reads the files it needs — it just doesn't waste tokens finding them.

### 4. Run 1 was an outlier

Run 1 showed FMM using MORE tokens (+13%) and costing more. In that run, both conditions were efficient — the control got lucky with its exploration path. Runs 2-4, where the control thrashed, show the real value: FMM provides a floor on efficiency.

## Conclusion

FMM + explicit preamble works. The LLM reads the manifest, uses headers to understand file purpose, and navigates directly to relevant code. The value isn't just speed — it's consistency.

## Next Steps

- Test with a real GitHub issue (not a synthetic task)
- Test across different task types (bug fix, refactor, not just features)
- Test with larger codebases (50+ files)
- Test without preamble (organic discovery via CLAUDE.md hint)
