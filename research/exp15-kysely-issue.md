# Experiment 15: Real OSS Issue — Kysely DROP COLUMN IF EXISTS

## Question

Does FMM (headers-only, no manifest) help an LLM solve a real GitHub issue on a popular OSS repo (279 TypeScript files)?

## Setup

**Repo:** [Kysely](https://github.com/kysely-org/kysely) — TypeScript SQL query builder, 13.4k stars, 279 source files.

**Issue:** [#1623](https://github.com/kysely-org/kysely/issues/1623) — Add `DROP COLUMN IF EXISTS` support to `AlterTableBuilder`.

**Task prompt:**
> Implement DROP COLUMN IF EXISTS support for Kysely (GitHub issue #1623). The ALTER TABLE builder should support `dropColumn('col').ifExists()` similar to how `DROP TABLE IF EXISTS` works. Look at drop-table-node.ts and drop-table-builder.ts for the IF EXISTS pattern, then apply it to drop-column-node.ts and alter-table-builder.ts. Update the query compiler and add tests.

**FMM preamble (headers-only, no manifest):**
```
Files in this codebase contain FMM headers — structured metadata blocks
at the top of each file listing exports, imports, and dependencies.
Read these headers first to understand file purpose before reading full content.
Use Grep to search FMM headers across files to find what you need.
```

**Model:** Sonnet, $3 budget cap, 30 max turns.

**Isolation:** Claude CLI with `--setting-sources ""`, `--strict-mcp-config`, no MCP, no session persistence.

## Results

### Run 1 (invalidated — used .fmm/index.json manifest)

| Metric | Clean | FMM | Delta |
|---|---|---|---|
| Tool calls | 40 | 42 | +5% |
| Files read | 10 | 12 | +20% |
| Glob calls | 1 | 0 | -100% |
| Grep calls | 11 | 8 | -27% |
| Read calls | 14 | 18 | +29% |
| Tokens (k) | 919 | 1,252 | +36% |
| Cost ($) | $0.55 | $0.71 | +30% |

> Invalidated: FMM condition read `.fmm/index.json` (7,852 lines for 279 files), consuming significant tokens. Manifest approach has been removed from FMM.

### Run 2 (headers-only — correct approach)

| Metric | Clean | FMM | Delta |
|---|---|---|---|
| Tool calls | 39 | 37 | **-5%** |
| Files read | 9 | 8 | **-11%** |
| Glob calls | 6 | 0 | **-100%** |
| Grep calls | 10 | 16 | +60% |
| Read calls | 13 | 10 | **-23%** |
| Edit calls | 9 | 8 | -11% |
| Tokens (k) | 1,017 | 1,186 | +17% |
| Cost ($) | $0.56 | $0.66 | +19% |
| Turns | 31 | 31 | 0% |
| Wall (s) | 152 | 188 | +24% |

## Analysis

### FMM eliminated blind directory searching

The most striking result: **FMM drove Glob calls to zero** (6 → 0). The clean condition used 6 Glob calls to discover file structure. The FMM condition never needed to — it used Grep on FMM headers instead. This confirms FMM changes the navigation pattern from "list directories, then look" to "search headers for what I need."

### Navigation shifted from Glob to Grep

FMM used 60% more Grep calls (16 vs 10). It was searching FMM header metadata (exports, dependencies) via Grep to find relevant files, replacing the Glob→Read→evaluate cycle. This is the intended behavior — headers as a structured search index.

### Fewer Read calls, fewer files read

FMM read fewer files (8 vs 9) and made fewer Read calls (13 vs 10, -23%). The headers let it skip files it didn't need. The clean condition had to Read files to determine relevance; FMM could determine relevance from Grep results on headers.

### Cost was still higher (+19%)

Despite fewer tool calls and file reads, FMM cost more. The extra Grep calls on files with FMM headers returned more content (each file has 5-10 extra header lines). Over 16 Grep calls on 279 files with headers, the cumulative token cost adds up.

### Both hit max turns

Both conditions used all 31 turns. The task was complex enough to exhaust the turn budget regardless of navigation efficiency. A higher turn budget might show divergence.

## Comparison with Exp14

| Metric | Exp14 (18 files) | Exp15 (279 files) |
|---|---|---|
| Glob reduction | -65% | -100% |
| Cost delta | -50% | +19% |
| Token delta | -59% | +17% |
| Tool call delta | -19% | -5% |

FMM's navigation benefit scales — Glob elimination was even stronger on the larger repo. But the **token cost of header metadata in Grep results** scales too, and on this well-specified task it wasn't offset by navigation savings.

## Hypotheses

1. **Task specificity matters**: The prompt named specific files to reference. A vague prompt ("fix the bug where column drops fail") would force more navigation, where FMM's Glob elimination provides greater value.
2. **Header verbosity**: FMM headers on a 279-file repo add ~1,400 lines of metadata (5 lines × 279 files). Grep hits on these inflate token counts. Shorter headers or selective header insertion could help.
3. **n=1 limitation**: Exp14 showed 4.2x variance in control behavior. One run is insufficient to draw conclusions.

## Next Steps

- Run 3-4 more iterations for statistical significance
- Test with a vague task prompt (no file hints)
- Test with a different task type (bug fix, refactor)
- Measure header size impact: full headers vs minimal headers
