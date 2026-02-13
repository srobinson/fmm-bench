#!/usr/bin/env bash
set -euo pipefail

# ============================================================================
# FMM Issue Experiment
# Test FMM on a real GitHub issue against a real OSS repo.
# Usage: ./run-issue.sh <clean-repo> <fmm-repo> "<task-prompt>"
#
# FMM condition: sidecars present + preamble in prompt
# Clean condition: no sidecars, no preamble, identical tools/flags
# ============================================================================

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
CLEAN_SRC="${1:?Usage: $0 <clean-repo-dir> <fmm-repo-dir> '<task-prompt>'}"
FMM_SRC="${2:?Usage: $0 <clean-repo-dir> <fmm-repo-dir> '<task-prompt>'}"
TASK_PROMPT="${3:?Usage: $0 <clean-repo-dir> <fmm-repo-dir> '<task-prompt>'}"

RUN_DIR="$SCRIPT_DIR/runs/$(date +%Y%m%d-%H%M%S)"
MODEL="${MODEL:-sonnet}"
MAX_BUDGET="${MAX_BUDGET:-2.00}"
MAX_TURNS="${MAX_TURNS:-30}"
FMM_BIN="${FMM_BIN:-fmm}"

log()  { echo -e "\033[0;34m[exp]\033[0m $*"; }
ok()   { echo -e "\033[0;32m[ok]\033[0m $*"; }
err()  { echo -e "\033[0;31m[err]\033[0m $*" >&2; }

command -v claude >/dev/null 2>&1 || { err "claude CLI not found"; exit 1; }
command -v jq    >/dev/null 2>&1 || { err "jq not found"; exit 1; }
[[ -d "$CLEAN_SRC" ]] || { err "Clean repo not found: $CLEAN_SRC"; exit 1; }
[[ -d "$FMM_SRC" ]]   || { err "FMM repo not found: $FMM_SRC"; exit 1; }

mkdir -p "$RUN_DIR"
log "Run dir:    $RUN_DIR"
log "Model:      $MODEL"
log "Budget:     \$$MAX_BUDGET per condition"
log "Max turns:  $MAX_TURNS"
log "FMM bin:    $FMM_BIN"
log "Task:       ${TASK_PROMPT:0:80}..."

# ---- Copy repos ----
for variant in clean fmm; do
    DEST="$RUN_DIR/$variant"
    if [[ "$variant" == "clean" ]]; then
        cp -R "$CLEAN_SRC" "$DEST"
    else
        cp -R "$FMM_SRC" "$DEST"
    fi
    if [[ ! -d "$DEST/.git" ]]; then
        git -C "$DEST" init -q
        git -C "$DEST" add -A
        git -C "$DEST" commit -q -m "initial" --allow-empty
    fi
done

# ---- FMM preamble (prepended to task prompt for FMM condition) ----
FMM_PREAMBLE="This project has .fmm sidecar files next to every source file. Before reading any source file, search sidecars first:
- Grep \"exports:.*SymbolName\" **/*.fmm to find where something is defined
- Grep \"dependencies:.*filename\" **/*.fmm to find what depends on a file
- Read the .fmm sidecar to see exports, imports, dependencies, LOC
- Only open source files you will actually edit

"

ok "Workspaces ready"

# ---- Run one condition ----
run_condition() {
    local variant="$1"
    local work_dir="$RUN_DIR/$variant"
    local out_jsonl="$RUN_DIR/${variant}-stream.jsonl"

    log "Running \033[1m${variant}\033[0m condition..."
    local t0
    t0=$(date +%s)

    local prompt="$TASK_PROMPT"
    if [[ "$variant" == "fmm" ]]; then
        prompt="${FMM_PREAMBLE}${TASK_PROMPT}"
    fi

    # Both conditions: identical flags, only the prompt differs
    # Tee stream to live log — shows tool calls in real time
    (cd "$work_dir" && claude \
        -p "$prompt" \
        --output-format stream-json \
        --verbose \
        --model "$MODEL" \
        --dangerously-skip-permissions \
        --setting-sources "" \
        --disable-slash-commands \
        --strict-mcp-config \
        --mcp-config '{"mcpServers":{}}' \
        --no-session-persistence \
        --max-turns "$MAX_TURNS" \
        --max-budget-usd "$MAX_BUDGET" \
    ) | tee "$out_jsonl" | jq -r --unbuffered '
        if .type == "assistant" then
            .message.content[]? |
            if .type == "tool_use" then
                "  → \(.name)(\(.input | to_entries | map("\(.key)=\(.value | tostring | .[0:60])") | join(", ")))"
            elif .type == "text" then
                "  ✎ \(.text | split("\n")[0] | .[0:100])"
            else empty end
        elif .type == "result" then
            "  ✓ done — \(.num_turns // "?") turns, $\(.total_cost_usd // "?" | tostring | .[0:6])"
        else empty end
    ' 2>"$RUN_DIR/${variant}-stderr.log" || true

    local t1
    t1=$(date +%s)
    local wall=$(( t1 - t0 ))
    echo "$wall" > "$RUN_DIR/${variant}-wall.txt"
    ok "$variant done in ${wall}s ($(wc -l < "$out_jsonl" | tr -d ' ') stream lines)"
}

# ---- Extract metrics ----
extract_metrics() {
    local variant="$1"
    local jsonl="$RUN_DIR/${variant}-stream.jsonl"
    local out="$RUN_DIR/${variant}-metrics.json"

    if [[ ! -s "$jsonl" ]]; then
        echo '{"error":"no output"}' > "$out"
        return
    fi

    jq -s --arg wall "$(cat "$RUN_DIR/${variant}-wall.txt" 2>/dev/null || echo 0)" '
    [.[] | select(.type? == "assistant") | .message.content[]? | select(.type? == "tool_use")] as $tools |
    [$tools[] | .name] as $names |
    ($names | group_by(.) | map({key: .[0], value: length}) | from_entries) as $breakdown |
    ([$tools[] | select(.name == "Read") | .input.file_path // empty] | unique) as $files_read |
    ([.[] | select(.type? == "result")] | last // {}) as $result |
    ($result.usage // {}) as $usage |
    (($usage.input_tokens // 0) + ($usage.cache_read_input_tokens // 0) + ($usage.cache_creation_input_tokens // 0)) as $in_tokens |
    ($usage.output_tokens // 0) as $out_tokens |
    {
      total_tool_calls: ($names | length),
      tool_breakdown: $breakdown,
      files_read_count: ($files_read | length),
      files_read: $files_read,
      input_tokens: ($in_tokens / 1000 | round),
      output_tokens: ($out_tokens / 1000 | round),
      total_tokens: (($in_tokens + $out_tokens) / 1000 | round),
      cost_usd: ($result.total_cost_usd // 0),
      duration_ms: ($result.duration_ms // 0),
      num_turns: ($result.num_turns // 0),
      wall_seconds: ($wall | tonumber? // 0)
    }' "$jsonl" > "$out" 2>/dev/null || echo '{"error":"parse failed"}' > "$out"
}

# ---- Report ----
generate_report() {
    local clean="$RUN_DIR/clean-metrics.json"
    local fmm="$RUN_DIR/fmm-metrics.json"
    local report="$RUN_DIR/report.txt"

    jq -n --slurpfile c "$clean" --slurpfile f "$fmm" '
    ($c[0]) as $c | ($f[0]) as $f |
    def pct: if .[0] == 0 then "N/A" else ((.[1] - .[0]) / .[0] * 100 | round | tostring) + "%" end;
    {
      clean: $c,
      fmm: $f,
      delta: {
        tool_calls: ([$c.total_tool_calls, $f.total_tool_calls] | pct),
        files_read: ([$c.files_read_count, $f.files_read_count] | pct),
        total_tokens: ([$c.total_tokens, $f.total_tokens] | pct),
        cost: ([$c.cost_usd, $f.cost_usd] | pct),
        turns: ([$c.num_turns, $f.num_turns] | pct)
      }
    }' > "$RUN_DIR/results.json"

    echo "============================================================" > "$report"
    echo "  FMM ISSUE EXPERIMENT — RESULTS" >> "$report"
    echo "============================================================" >> "$report"
    echo "" >> "$report"
    echo "  Model:  $MODEL" >> "$report"
    echo "  Task:   ${TASK_PROMPT:0:100}..." >> "$report"
    echo "  Repo:   $(basename "$CLEAN_SRC")" >> "$report"
    echo "  FMM:    sidecars + preamble (same tools, prompt differs)" >> "$report"
    echo "  Clean:  no sidecars, no preamble" >> "$report"
    echo "" >> "$report"

    jq -rn --slurpfile c "$clean" --slurpfile f "$fmm" '
    ($c[0]) as $c | ($f[0]) as $f |
    def delta: if .[0] == 0 then (if .[1] == 0 then "  0%" else "  N/A" end) elif .[1] < .[0] then " " + (((.[1] - .[0]) / .[0] * 100 | round | tostring) + "%") else "+" + (((.[1] - .[0]) / .[0] * 100 | round | tostring) + "%") end;
    ($c.tool_breakdown) as $cb | ($f.tool_breakdown) as $fb |
    "  Metric                       Clean       FMM         Delta",
    "  -------------------------  ---------  ---------  ---------",
    "  Tool calls                 \($c.total_tool_calls | tostring | .[0:9] | . + " " * (9 - length))  \($f.total_tool_calls | tostring | .[0:9] | . + " " * (9 - length))  \([$c.total_tool_calls, $f.total_tool_calls] | delta)",
    "    Read                     \(($cb.Read // 0) | tostring | .[0:9] | . + " " * (9 - length))  \(($fb.Read // 0) | tostring | .[0:9] | . + " " * (9 - length))  \([($cb.Read // 0), ($fb.Read // 0)] | delta)",
    "    Glob                     \(($cb.Glob // 0) | tostring | .[0:9] | . + " " * (9 - length))  \(($fb.Glob // 0) | tostring | .[0:9] | . + " " * (9 - length))  \([($cb.Glob // 0), ($fb.Glob // 0)] | delta)",
    "    Grep                     \(($cb.Grep // 0) | tostring | .[0:9] | . + " " * (9 - length))  \(($fb.Grep // 0) | tostring | .[0:9] | . + " " * (9 - length))  \([($cb.Grep // 0), ($fb.Grep // 0)] | delta)",
    "    Edit                     \(($cb.Edit // 0) | tostring | .[0:9] | . + " " * (9 - length))  \(($fb.Edit // 0) | tostring | .[0:9] | . + " " * (9 - length))  \([($cb.Edit // 0), ($fb.Edit // 0)] | delta)",
    "    Bash                     \(($cb.Bash // 0) | tostring | .[0:9] | . + " " * (9 - length))  \(($fb.Bash // 0) | tostring | .[0:9] | . + " " * (9 - length))  \([($cb.Bash // 0), ($fb.Bash // 0)] | delta)",
    "    MCP                      \(($cb.mcp // 0) | tostring | .[0:9] | . + " " * (9 - length))  \(($fb.mcp // 0) | tostring | .[0:9] | . + " " * (9 - length))  \([($cb.mcp // 0), ($fb.mcp // 0)] | delta)",
    "  Files read                 \($c.files_read_count | tostring | .[0:9] | . + " " * (9 - length))  \($f.files_read_count | tostring | .[0:9] | . + " " * (9 - length))  \([$c.files_read_count, $f.files_read_count] | delta)",
    "  Tokens (k)                 \($c.total_tokens | tostring | .[0:9] | . + " " * (9 - length))  \($f.total_tokens | tostring | .[0:9] | . + " " * (9 - length))  \([$c.total_tokens, $f.total_tokens] | delta)",
    "  Cost ($)                   \($c.cost_usd | tostring | .[0:9] | . + " " * (9 - length))  \($f.cost_usd | tostring | .[0:9] | . + " " * (9 - length))  \([$c.cost_usd, $f.cost_usd] | delta)",
    "  API turns                  \($c.num_turns | tostring | .[0:9] | . + " " * (9 - length))  \($f.num_turns | tostring | .[0:9] | . + " " * (9 - length))  \([$c.num_turns, $f.num_turns] | delta)",
    "",
    "  Files read (FMM):  \($f.files_read | join(", "))",
    "",
    "  Files read (Clean): \($c.files_read | join(", "))"
    ' >> "$report"

    cat "$report"
}

# ---- Main ----
log "Starting FMM issue experiment..."
echo ""

# Run FMM first — validate it works before spending on control
run_condition fmm
run_condition clean

log "Extracting metrics..."
extract_metrics clean
extract_metrics fmm

echo ""
generate_report

echo ""
log "Results:  $RUN_DIR/results.json"
log "Traces:   $RUN_DIR/{clean,fmm}-stream.jsonl"
log "Done."
