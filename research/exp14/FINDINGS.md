# Experiment 14: Manifest Discovery Without Instructions

## Question

Do LLMs discover and use `.fmm/index.json` organically during codebase exploration, without being told about it?

## TL;DR

**No.** LLMs do not discover the manifest without explicit instructions. But with full CLAUDE.md instructions, manifest usage reduces tool calls by **91%** and achieves **100% accuracy**. A one-line hint is insufficient.

**Recommendation:** fmm must ship a CLAUDE.md integration that installs instructions automatically. The manifest alone is invisible to LLMs.

## Methodology

### Test Codebase

18-file TypeScript authentication app (JWT, sessions, RBAC, middleware, API layer). Realistic import/export graph with 25+ auth-related exports across 9 files.

### Task

> "Find all files that export authentication-related functions. List each file path and the specific exports."

### Conditions

| Condition | Description | Runs |
|---|---|---|
| **Control** | Clean codebase, no fmm artifacts | 3 |
| **Inline** | Frontmatter comment blocks in files, no manifest | 3 |
| **Manifest** | `.fmm/index.json` present, no inline comments | 3 |
| **Hint** | Manifest + one-line CLAUDE.md: "Check .fmm/ for codebase index" | 3 |
| **Full** | Manifest + full CLAUDE.md with fmm navigation instructions | 3 |

### Isolation

- Claude CLI with `--system-prompt` override (no fmm knowledge in system prompt)
- User `~/.claude/CLAUDE.md` temporarily hidden during non-full runs
- `--no-session-persistence` (no state leakage between runs)
- `--disable-slash-commands` (no skill leakage)
- Model: Claude Sonnet 4.5 (`claude-sonnet-4-5-20250929`)
- Stream-JSON output for full tool call tracing

**Confound noted:** MCP servers (mcp-files, context7, linear-server) remained active. Their tool descriptions appeared in the tool list but did not affect behavior — no MCP tools were invoked during experiments.

## Results

### Summary Table

| Condition | Avg Tool Calls | Avg Files Read | Avg Tokens (in+out) | Avg Cost | Discovered FMM | Used Manifest | Avg Accuracy |
|---|---|---|---|---|---|---|---|
| Control | 10.3 | 7.3 | 81,341 | $0.088 | 0/3 | 0/3 | 6.3/7 |
| Inline | 13.3 | 7.7 | 58,652 | $0.086 | 0/3 | 0/3 | 6.0/7 |
| Manifest | 11.7 | 8.0 | 90,333 | $0.097 | 0/3 | 0/3 | 6.7/7 |
| Hint (1-line) | 6.7 | 4.7 | 50,484 | $0.069 | 0/3 | 0/3 | 5.7/7 |
| **Full CLAUDE.md** | **1.0** | **1.0** | **30,627** | **$0.046** | **3/3** | **3/3** | **7.0/7** |

### Per-Run Data

#### Control (clean/)
| Run | Tool Calls | Files Read | Tokens In | Tokens Out | Cost | Accuracy |
|---|---|---|---|---|---|---|
| 1 | 11 | 7 | 86,548 | 1,577 | $0.090 | 6/7 |
| 2 | 12 | 9 | 89,479 | 1,825 | $0.100 | 7/7 |
| 3 | 8 | 6 | 63,287 | 1,308 | $0.075 | 6/7 |

#### Inline (inline/)
| Run | Tool Calls | Files Read | Tokens In | Tokens Out | Cost | Accuracy |
|---|---|---|---|---|---|---|
| 1 | 13 | 8 | 41,723 | 1,681 | $0.083 | 6/7 |
| 2 | 17 | 9 | 87,665 | 2,016 | $0.107 | 7/7 |
| 3 | 10 | 6 | 41,635 | 1,237 | $0.068 | 5/7 |

#### Manifest (manifest/)
| Run | Tool Calls | Files Read | Tokens In | Tokens Out | Cost | Accuracy |
|---|---|---|---|---|---|---|
| 1 | 11 | 7 | 86,477 | 1,613 | $0.090 | 6/7 |
| 2 | 11 | 8 | 88,464 | 1,733 | $0.096 | 7/7 |
| 3 | 13 | 9 | 90,730 | 1,983 | $0.104 | 7/7 |

#### Hint (manifest + 1-line CLAUDE.md)
| Run | Tool Calls | Files Read | Tokens In | Tokens Out | Cost | Accuracy |
|---|---|---|---|---|---|---|
| 1 | 9 | 8 | 46,432 | 1,393 | $0.085 | 6/7 |
| 2 | 10 | 6 | 72,313 | 1,420 | $0.091 | 6/7 |
| 3 | 1 | 0 | 29,474 | 420 | $0.032 | 5/7 |

#### Full CLAUDE.md (manifest + full instructions)
| Run | Tool Calls | Files Read | Tokens In | Tokens Out | Cost | Accuracy |
|---|---|---|---|---|---|---|
| 1 | 1 | 1 | 30,141 | 514 | $0.047 | 7/7 |
| 2 | 1 | 1 | 30,141 | 448 | $0.046 | 7/7 |
| 3 | 1 | 1 | 30,145 | 492 | $0.047 | 7/7 |

## Key Findings

### 1. LLMs Do NOT Discover `.fmm/` Organically

**0 out of 9 baseline runs** discovered or used the `.fmm/` directory. Even though the manifest variant had `.fmm/index.json` sitting right there, Claude never listed hidden directories, never explored `.fmm/`, and never read `index.json`.

LLMs follow a consistent exploration pattern for this task:
1. Grep for auth-related patterns
2. Glob for files with "auth" in the name
3. Read each candidate file to extract exports

This pattern works but is brute-force. The manifest was invisible.

### 2. Inline Frontmatter Comments Are Invisible Too

The inline variant (frontmatter comment blocks at the top of each file) showed **no measurable improvement** over the clean control. Claude reads files to find exports, and the frontmatter comments at the top of files are indistinguishable from regular comments — Claude doesn't treat them as structured metadata.

| Metric | Control | Inline | Delta |
|---|---|---|---|
| Tool calls | 10.3 | 13.3 | +29% (worse) |
| Files read | 7.3 | 7.7 | +5% |
| Accuracy | 6.3/7 | 6.0/7 | -5% |

Inline comments added no value without explicit instructions to use them.

### 3. A One-Line Hint Is Insufficient

The minimal CLAUDE.md hint ("Check .fmm/ for codebase index") did **not** cause Claude to use the manifest in any of 3 runs. Claude ignored the hint and used its standard exploration patterns.

This suggests that a vague directional hint is too weak. Claude doesn't explore `.fmm/` just because someone mentions it exists — it needs to understand *why* and *how* to use it.

### 4. Full CLAUDE.md Instructions Are Transformative

With proper CLAUDE.md instructions (explaining the manifest structure, how to query it, and when to use it):

- **91% fewer tool calls** (1.0 vs 11.7 baseline)
- **87% fewer files read** (1.0 vs 8.0 baseline)
- **66% fewer tokens** (30,627 vs 90,333 baseline)
- **53% lower cost** ($0.046 vs $0.097 baseline)
- **100% accuracy** (7/7 vs 6.7/7 baseline)

Claude's behavior with full instructions:
1. Read `.fmm/index.json` (1 tool call)
2. Parse the export index to find auth-related exports
3. Return complete, accurate answer directly from manifest data

Zero file exploration needed. The manifest contained everything.

### 5. Contamination Discovery

During initial testing (before proper isolation), we discovered that `~/.claude/CLAUDE.md` contains fmm navigation instructions. When these leaked into the experiment, Claude proactively read `.fmm/index.json` as its **first action** — even on the clean variant where the file doesn't exist.

This confirms: CLAUDE.md instructions are loaded and acted upon immediately. The instruction mechanism is highly effective.

## Answers to Research Questions

### Do LLMs discover `.fmm/` during normal exploration?
**No.** 0/9 runs across all baseline conditions. The `.fmm/` directory is invisible during standard codebase exploration.

### Do LLMs query `index.json` when they find `.fmm/`?
**N/A for organic discovery** (never happened). When instructed via CLAUDE.md, they read it immediately and use it effectively (3/3 runs).

### Is manifest sufficient alone, or need CLAUDE.md hint?
**Manifest is NOT sufficient alone.** CLAUDE.md instructions are required. Furthermore, a minimal one-line hint is not enough — the instructions need to explain the manifest's purpose and structure.

## Recommendation

### What fmm Should Ship

1. **Manifest generation** (`.fmm/index.json`) — the core value. When the LLM knows to use it, performance improvement is dramatic.

2. **Automatic CLAUDE.md integration** — `fmm init` should add fmm navigation instructions to the project's CLAUDE.md (or create one). This is the critical enabler. Without it, the manifest provides zero value.

3. **The CLAUDE.md content should be:**
   - Explicit about checking `.fmm/index.json` first
   - Clear about the manifest's structure (exportIndex, files, dependencies)
   - Brief (the full instructions work, but could be distilled to ~5 lines)

4. **Do NOT rely on inline comments** — they provide no measurable benefit without instructions, and they modify source files (higher friction than a manifest).

### Architecture Implication

The manifest + CLAUDE.md approach is strictly superior to inline comments:
- Manifest: single file, no source modifications, easy to regenerate
- CLAUDE.md: standard convention, zero friction, one-time setup
- Together: 91% reduction in tool calls, 100% accuracy

The inline frontmatter approach from earlier experiments only worked when CLAUDE.md explicitly told the LLM to use it. The same is true for manifests. **The transport mechanism (inline vs manifest) doesn't matter — the instruction mechanism (CLAUDE.md) is what matters.**

## Next Steps

1. Build manifest generation CLI (`fmm generate` -> `.fmm/index.json`)
2. Build CLAUDE.md integration (`fmm init` -> adds instructions to CLAUDE.md)
3. Test with larger codebases (100+ files) to validate scaling
4. Test with other LLM tools (Cursor, Cody, etc.) that have similar config mechanisms
5. Optimize CLAUDE.md instructions — find the minimum effective instruction set
