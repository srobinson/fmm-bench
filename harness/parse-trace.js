#!/usr/bin/env node

const fs = require("fs");

const [rawPath, tracePath, variant, task, duration, hint] = process.argv.slice(2);

if (!rawPath || !tracePath) {
  console.error("Usage: parse-trace.js <raw.jsonl> <trace.json> <variant> <task> <duration> <hint>");
  process.exit(1);
}

const raw = fs.readFileSync(rawPath, "utf8").trim();
const lines = raw.split("\n").filter(Boolean);
const events = [];

for (const line of lines) {
  try {
    events.push(JSON.parse(line));
  } catch {
    // skip malformed lines
  }
}

// Extract tool calls from assistant messages
const toolCalls = [];
const filesRead = new Set();
const globPatterns = [];
const grepPatterns = [];
let totalLinesRead = 0;
let discoveredFmm = false;
let intentionallyExploredFmm = false;
let usedManifest = false;
let fmmInToolOutput = false;
let finalAnswer = "";

for (const event of events) {
  if (event.type === "assistant" && event.message?.content) {
    for (const block of event.message.content) {
      if (block.type === "tool_use") {
        const call = {
          tool: block.name,
          id: block.id,
          input: block.input,
        };
        toolCalls.push(call);

        // Track file reads
        if (block.name === "Read" && block.input?.file_path) {
          const filePath = block.input.file_path;
          filesRead.add(filePath);
          const limit = block.input.limit || 2000;
          totalLinesRead += limit;

          // Check if they read the fmm manifest
          if (filePath.includes(".fmm/index.json")) {
            usedManifest = true;
            intentionallyExploredFmm = true;
            discoveredFmm = true;
          }
          if (filePath.includes(".fmm")) {
            intentionallyExploredFmm = true;
            discoveredFmm = true;
          }
        }

        // Track glob patterns
        if (block.name === "Glob" && block.input?.pattern) {
          globPatterns.push(block.input.pattern);
          if (block.input.pattern.includes(".fmm") || block.input.pattern.includes("fmm")) {
            intentionallyExploredFmm = true;
            discoveredFmm = true;
          }
        }

        // Track grep patterns
        if (block.name === "Grep" && block.input?.pattern) {
          grepPatterns.push(block.input.pattern);
          if (block.input.pattern.includes("fmm")) {
            discoveredFmm = true;
          }
        }

        // Track Bash commands that explore fmm
        if (block.name === "Bash" && block.input?.command) {
          const cmd = block.input.command;
          if (cmd.includes(".fmm") || cmd.includes("fmm")) {
            intentionallyExploredFmm = true;
            discoveredFmm = true;
          }
          if (cmd.includes("cat") && cmd.includes("index.json") && cmd.includes(".fmm")) {
            usedManifest = true;
          }
        }
      }

      // Capture final text response
      if (block.type === "text") {
        finalAnswer = block.text;
      }
    }
  }
}

// Extract token usage from result event
const resultEvent = events.find((e) => e.type === "result");
const usage = resultEvent?.usage || {};
const tokensIn = (usage.input_tokens || 0) + (usage.cache_read_input_tokens || 0);
const tokensOut = usage.output_tokens || 0;
const costUsd = resultEvent?.total_cost_usd || 0;

// Also check tool results for fmm discovery
for (const event of events) {
  if (event.type === "tool_result" || event.type === "user") {
    // Tool results come back in user messages in stream-json
    const content = JSON.stringify(event);
    if (content.includes(".fmm")) {
      fmmInToolOutput = true;
      discoveredFmm = true;
    }
  }
}

const trace = {
  variant,
  task,
  hint: hint === "1",
  duration_seconds: parseInt(duration, 10) || 0,
  tool_calls: toolCalls,
  tool_calls_count: toolCalls.length,
  files_read: [...filesRead],
  total_lines_read: totalLinesRead,
  glob_patterns: globPatterns,
  grep_patterns: grepPatterns,
  discovered_fmm: discoveredFmm,
  intentionally_explored_fmm: intentionallyExploredFmm,
  fmm_in_tool_output: fmmInToolOutput,
  used_manifest: usedManifest,
  tokens_in: tokensIn,
  tokens_out: tokensOut,
  cost_usd: Math.round(costUsd * 1000000) / 1000000,
  final_answer: finalAnswer,
  num_turns: resultEvent?.num_turns || 0,
  model_usage: resultEvent?.modelUsage || {},
};

fs.writeFileSync(tracePath, JSON.stringify(trace, null, 2));
console.log(`  Parsed ${events.length} events, ${toolCalls.length} tool calls`);
