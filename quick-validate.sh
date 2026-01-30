#!/usr/bin/env bash
set -euo pipefail

# ============================================================================
# FMM Quick Validation Experiment
# Does FMM (frontmatter metadata) help LLMs navigate codebases?
#
# Design: 2 conditions (clean vs fmm), same coding task, compare metrics.
# Measures: tool calls, files read, tokens, cost, wall time.
# ============================================================================

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
REPOS_SRC="/Users/alphab/Dev/LLM/DEV/fmm-worktrees/nancy-ALP-319/research/exp14/repos"
RUN_DIR="$SCRIPT_DIR/runs/$(date +%Y%m%d-%H%M%S)"
MODEL="${MODEL:-sonnet}"
MAX_BUDGET="${MAX_BUDGET:-2.00}"

TASK_PROMPT='Add refresh token support to the authentication system. Generate refresh tokens on login alongside access tokens. Store refresh tokens with expiry (7 days) in the user session. Add a /auth/refresh endpoint that issues new access tokens. Invalidate refresh token on logout.'

FMM_PREAMBLE='Files in this codebase contain FMM headers — structured metadata blocks at the top of each file listing exports, imports, and dependencies. Read these headers first to understand file purpose before reading full content. A manifest is available at .fmm/index.json indexing all files.

'

GREEN='\033[0;32m'
BLUE='\033[0;34m'
YELLOW='\033[1;33m'
RED='\033[0;31m'
BOLD='\033[1m'
NC='\033[0m'

log()  { echo -e "${BLUE}[exp]${NC} $*"; }
warn() { echo -e "${YELLOW}[warn]${NC} $*"; }
ok()   { echo -e "${GREEN}[ok]${NC} $*"; }
err()  { echo -e "${RED}[err]${NC} $*" >&2; }

# ---- Preflight ----
command -v claude >/dev/null 2>&1 || { err "claude CLI not found"; exit 1; }
command -v jq    >/dev/null 2>&1 || { err "jq not found"; exit 1; }
[[ -d "$REPOS_SRC/clean" ]] || { err "Clean repo not found at $REPOS_SRC/clean"; exit 1; }
[[ -d "$REPOS_SRC/fmm" ]]   || { err "FMM repo not found at $REPOS_SRC/fmm"; exit 1; }

mkdir -p "$RUN_DIR"
log "Run dir:  $RUN_DIR"
log "Model:    $MODEL"
log "Budget:   \$$MAX_BUDGET per condition"

# ---- Copy repos into isolated workspaces with their own .git ----
for variant in clean fmm; do
    DEST="$RUN_DIR/$variant"
    cp -R "$REPOS_SRC/$variant" "$DEST"
    if [[ ! -d "$DEST/.git" ]]; then
        git -C "$DEST" init -q
        git -C "$DEST" add -A
        git -C "$DEST" commit -q -m "initial" --allow-empty
    fi
done
ok "Workspaces ready"

# ---- Run one condition ----
run_condition() {
    local variant="$1"
    local work_dir="$RUN_DIR/$variant"
    local out_jsonl="$RUN_DIR/${variant}-stream.jsonl"
    local prompt

    if [[ "$variant" == "fmm" ]]; then
        prompt="${FMM_PREAMBLE}${TASK_PROMPT}"
    else
        prompt="$TASK_PROMPT"
    fi

    log "Running ${BOLD}${variant}${NC} condition..."
    local t0
    t0=$(date +%s)

    # Key isolation flags:
    #   --verbose                     required for stream-json
    #   --dangerously-skip-permissions  no permission prompts
    #   --tools "..."                   restrict to navigation + editing
    #   --strict-mcp-config + empty     no MCP servers
    #   --setting-sources ""            ignore user/project CLAUDE.md
    #   --disable-slash-commands        no skills
    #   --no-session-persistence        ephemeral
    (cd "$work_dir" && claude \
        -p "$prompt" \
        --output-format stream-json \
        --verbose \
        --model "$MODEL" \
        --dangerously-skip-permissions \
        --tools "Read,Glob,Grep,Edit,Write,Bash" \
        --strict-mcp-config \
        --mcp-config '{"mcpServers":{}}' \
        --setting-sources "" \
        --disable-slash-commands \
        --no-session-persistence \
        --max-budget-usd "$MAX_BUDGET" \
    ) > "$out_jsonl" 2>"$RUN_DIR/${variant}-stderr.log" || true

    local t1
    t1=$(date +%s)
    local wall=$(( t1 - t0 ))
    echo "$wall" > "$RUN_DIR/${variant}-wall.txt"
    ok "$variant done in ${wall}s ($(wc -l < "$out_jsonl" | tr -d ' ') stream lines)"
}

# ---- Extract metrics from stream-json ----
extract_metrics() {
    local variant="$1"
    local jsonl="$RUN_DIR/${variant}-stream.jsonl"
    local out="$RUN_DIR/${variant}-metrics.json"

    if [[ ! -s "$jsonl" ]]; then
        warn "No output for $variant"
        echo '{"error":"no output"}' > "$out"
        return
    fi

    # The stream-json format from claude CLI:
    #   {"type":"system", ...}                             init message with tools list
    #   {"type":"assistant","message":{...content:[...]}}  assistant turns with tool_use blocks
    #   {"type":"tool_result", ...}                        tool results
    #   {"type":"result", ...}                             final summary with modelUsage

    # Strategy: parse all lines, extract tool_use blocks from assistant messages,
    # and grab the result summary.

    jq -s '
    # Tool use: assistant messages contain content arrays with tool_use items
    [.[] | select(.type? == "assistant") | .message.content[]? | select(.type? == "tool_use")] as $tools |

    [$tools[] | .name] as $names |

    ($names | group_by(.) | map({key: .[0], value: length}) | from_entries) as $breakdown |

    ([$tools[] | select(.name == "Read") | .input.file_path // empty] | unique) as $files_read |

    ([.[] | select(.type? == "result")] | last // {}) as $result |

    # modelUsage is keyed by model name; grab the first entry
    ($result.modelUsage // {} | to_entries | if length > 0 then .[0].value else {} end) as $mu |

    {
      total_tool_calls: ($names | length),
      tool_breakdown: $breakdown,
      read_calls: ($breakdown.Read // 0),
      grep_calls: ($breakdown.Grep // 0),
      glob_calls: ($breakdown.Glob // 0),
      edit_calls: ($breakdown.Edit // 0),
      write_calls: ($breakdown.Write // 0),
      bash_calls: ($breakdown.Bash // 0),
      files_read: $files_read,
      files_read_count: ($files_read | length),
      input_tokens: ($mu.inputTokens // 0),
      output_tokens: ($mu.outputTokens // 0),
      cache_read_tokens: ($mu.cacheReadInputTokens // 0),
      cache_creation_tokens: ($mu.cacheCreationInputTokens // 0),
      total_tokens: (($mu.inputTokens // 0) + ($mu.outputTokens // 0)),
      cost_usd: ($mu.costUSD // 0),
      duration_ms: ($result.duration_ms // $result.duration_api_ms // 0),
      num_turns: ($result.num_turns // 0),
      is_error: ($result.is_error // false)
    }
    ' "$jsonl" > "$out" 2>/dev/null

    if [[ ! -s "$out" ]] || ! jq empty "$out" 2>/dev/null; then
        warn "Primary jq extraction failed for $variant, using fallback..."
        fallback_extract "$variant"
    fi
}

fallback_extract() {
    local variant="$1"
    local jsonl="$RUN_DIR/${variant}-stream.jsonl"
    local out="$RUN_DIR/${variant}-metrics.json"

    # Simple grep-based counting
    local total read_c grep_c glob_c edit_c write_c bash_c
    total=$(grep -c '"type":"tool_use"\|"type": "tool_use"' "$jsonl" 2>/dev/null || echo 0)
    read_c=$(grep -c '"name":"Read"\|"name": "Read"' "$jsonl" 2>/dev/null || echo 0)
    grep_c=$(grep -c '"name":"Grep"\|"name": "Grep"' "$jsonl" 2>/dev/null || echo 0)
    glob_c=$(grep -c '"name":"Glob"\|"name": "Glob"' "$jsonl" 2>/dev/null || echo 0)
    edit_c=$(grep -c '"name":"Edit"\|"name": "Edit"' "$jsonl" 2>/dev/null || echo 0)
    write_c=$(grep -c '"name":"Write"\|"name": "Write"' "$jsonl" 2>/dev/null || echo 0)
    bash_c=$(grep -c '"name":"Bash"\|"name": "Bash"' "$jsonl" 2>/dev/null || echo 0)

    # Result line
    local rl
    rl=$(grep '"type":"result"\|"type": "result"' "$jsonl" | tail -1)
    local cost dur turns
    cost=$(echo "$rl" | jq '[.modelUsage // {} | to_entries[].value.costUSD // 0] | add' 2>/dev/null || echo 0)
    dur=$(echo "$rl" | jq '.duration_ms // .duration_api_ms // 0' 2>/dev/null || echo 0)
    turns=$(echo "$rl" | jq '.num_turns // 0' 2>/dev/null || echo 0)
    local itok otok
    itok=$(echo "$rl" | jq '[.modelUsage // {} | to_entries[].value.inputTokens // 0] | add' 2>/dev/null || echo 0)
    otok=$(echo "$rl" | jq '[.modelUsage // {} | to_entries[].value.outputTokens // 0] | add' 2>/dev/null || echo 0)

    jq -n \
        --argjson total "$total" \
        --argjson read_c "$read_c" \
        --argjson grep_c "$grep_c" \
        --argjson glob_c "$glob_c" \
        --argjson edit_c "$edit_c" \
        --argjson write_c "$write_c" \
        --argjson bash_c "$bash_c" \
        --argjson itok "$itok" \
        --argjson otok "$otok" \
        --argjson cost "$cost" \
        --argjson dur "$dur" \
        --argjson turns "$turns" \
        '{
          total_tool_calls: $total,
          read_calls: $read_c, grep_calls: $grep_c, glob_calls: $glob_c,
          edit_calls: $edit_c, write_calls: $write_c, bash_calls: $bash_c,
          files_read_count: 0, files_read: [],
          input_tokens: $itok, output_tokens: $otok,
          total_tokens: ($itok + $otok),
          cost_usd: $cost, duration_ms: $dur, num_turns: $turns
        }' > "$out"
}

# ---- Report ----
print_report() {
    local cm="$RUN_DIR/clean-metrics.json"
    local fm="$RUN_DIR/fmm-metrics.json"

    echo ""
    echo -e "${BOLD}============================================================${NC}"
    echo -e "${BOLD}       FMM QUICK VALIDATION — RESULTS${NC}"
    echo -e "${BOLD}============================================================${NC}"
    echo ""
    echo "  Model:     $MODEL"
    echo "  Run:       $RUN_DIR"
    echo "  Time:      $(date -u '+%Y-%m-%d %H:%M:%S UTC')"
    echo ""

    printf "  ${BOLD}%-28s %10s %10s %14s${NC}\n" "Metric" "Clean" "FMM" "Delta"
    printf "  %-28s %10s %10s %14s\n" "----------------------------" "----------" "----------" "--------------"

    row() {
        local label="$1" field="$2" fmt="${3:-%s}"
        local cv fv
        cv=$(jq -r ".$field // 0" "$cm" 2>/dev/null)
        fv=$(jq -r ".$field // 0" "$fm" 2>/dev/null)
        local delta=""
        if [[ "$cv" =~ ^-?[0-9]+\.?[0-9]*$ ]] && [[ "$fv" =~ ^-?[0-9]+\.?[0-9]*$ ]]; then
            local diff pct
            diff=$(awk "BEGIN{printf \"%.2f\", $fv - $cv}")
            if awk "BEGIN{exit ($cv == 0)}"; then
                pct=$(awk "BEGIN{printf \"%+.0f%%\", (($fv-$cv)/$cv)*100}")
                delta="$pct"
            fi
        fi
        printf "  %-28s %10s %10s %14s\n" "$label" "$cv" "$fv" "$delta"
    }

    row "Tool calls (total)"     "total_tool_calls"
    row "  Read"                  "read_calls"
    row "  Grep"                  "grep_calls"
    row "  Glob"                  "glob_calls"
    row "  Edit"                  "edit_calls"
    row "  Write"                 "write_calls"
    row "  Bash"                  "bash_calls"
    row "Files read (unique)"    "files_read_count"
    row "Input tokens"           "input_tokens"
    row "Output tokens"          "output_tokens"
    row "Total tokens"           "total_tokens"
    row "Cost (USD)"             "cost_usd"
    row "Duration (ms)"          "duration_ms"
    row "API turns"              "num_turns"

    # Wall clock
    local cw fw
    cw=$(cat "$RUN_DIR/clean-wall.txt" 2>/dev/null || echo "?")
    fw=$(cat "$RUN_DIR/fmm-wall.txt" 2>/dev/null || echo "?")
    printf "  %-28s %10s %10s %14s\n" "Wall clock (s)" "${cw}" "${fw}" ""

    echo ""

    # Files read comparison
    echo -e "  ${BOLD}Files read — Clean:${NC}"
    jq -r '.files_read[]? // empty' "$cm" 2>/dev/null | sed 's|.*/||' | sort | sed 's/^/    /' || echo "    (n/a)"
    echo ""
    echo -e "  ${BOLD}Files read — FMM:${NC}"
    jq -r '.files_read[]? // empty' "$fm" 2>/dev/null | sed 's|.*/||' | sort | sed 's/^/    /' || echo "    (n/a)"

    echo ""
    echo -e "${BOLD}============================================================${NC}"

    # Verdict
    local ct ft ctok ftok
    ct=$(jq -r '.total_tool_calls // 0' "$cm")
    ft=$(jq -r '.total_tool_calls // 0' "$fm")
    ctok=$(jq -r '.total_tokens // 0' "$cm")
    ftok=$(jq -r '.total_tokens // 0' "$fm")
    local ccost fcost
    ccost=$(jq -r '.cost_usd // 0' "$cm")
    fcost=$(jq -r '.cost_usd // 0' "$fm")

    echo ""
    local tool_pct tok_pct cost_pct
    tool_pct=$(awk "BEGIN{if($ct>0) printf \"%.0f\", (($ft-$ct)/$ct)*100; else print 0}")
    tok_pct=$(awk "BEGIN{if($ctok>0) printf \"%.0f\", (($ftok-$ctok)/$ctok)*100; else print 0}")
    cost_pct=$(awk "BEGIN{if($ccost+0>0) printf \"%.0f\", (($fcost-$ccost)/$ccost)*100; else print 0}")

    if [[ "$ft" -lt "$ct" ]] && [[ "$ftok" -lt "$ctok" ]]; then
        echo -e "  ${GREEN}${BOLD}SIGNAL: FMM helped.${NC}"
        echo -e "  Tool calls: ${tool_pct}% | Tokens: ${tok_pct}% | Cost: ${cost_pct}%"
    elif [[ "$ft" -lt "$ct" ]]; then
        echo -e "  ${YELLOW}${BOLD}SIGNAL: FMM may have helped.${NC} Fewer tool calls (${tool_pct}%) but more tokens."
    elif [[ "$ftok" -lt "$ctok" ]]; then
        echo -e "  ${YELLOW}${BOLD}SIGNAL: FMM may have helped.${NC} Fewer tokens (${tok_pct}%) but similar/more tool calls."
    elif [[ "$ft" -eq "$ct" ]] && [[ "$ftok" -eq "$ctok" ]]; then
        echo -e "  ${YELLOW}${BOLD}SIGNAL: No measurable difference.${NC}"
    else
        echo -e "  ${RED}${BOLD}SIGNAL: FMM did not help (or hurt).${NC}"
        echo -e "  Tool calls: ${tool_pct}% | Tokens: ${tok_pct}% | Cost: ${cost_pct}%"
    fi
    echo ""
    echo -e "  ${YELLOW}n=1 — run multiple times or with MODEL=opus for confidence.${NC}"
    echo ""
}

# ---- Main ----
main() {
    log "Starting FMM quick validation..."
    echo ""

    run_condition "clean"
    echo ""
    run_condition "fmm"
    echo ""

    log "Extracting metrics..."
    extract_metrics "clean"
    extract_metrics "fmm"

    print_report | tee "$RUN_DIR/report.txt"

    # Combined JSON for programmatic use
    jq -n \
        --slurpfile c "$RUN_DIR/clean-metrics.json" \
        --slurpfile f "$RUN_DIR/fmm-metrics.json" \
        --arg model "$MODEL" \
        --arg ts "$(date -u +%Y-%m-%dT%H:%M:%SZ)" \
        --arg cwall "$(cat "$RUN_DIR/clean-wall.txt" 2>/dev/null)" \
        --arg fwall "$(cat "$RUN_DIR/fmm-wall.txt" 2>/dev/null)" \
        '{
          experiment: "fmm-quick-validate",
          model: $model,
          timestamp: $ts,
          clean: ($c[0] + {wall_seconds: ($cwall|tonumber)}),
          fmm: ($f[0] + {wall_seconds: ($fwall|tonumber)})
        }' > "$RUN_DIR/results.json"

    log "Results JSON: $RUN_DIR/results.json"
    log "Stream logs:  $RUN_DIR/{clean,fmm}-stream.jsonl"
    log "Done."
}

main "$@"
