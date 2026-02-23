#!/usr/bin/env node
/**
 * Parses vitest snapshot mismatches from integration test logs.
 * Extracts expected vs received HTML for analysis.
 *
 * Usage: node scripts/parse-snapshot-mismatches.mjs [log-file] [output-file]
 * Defaults: .integration-tests/logs/nuxt-ui/verter-test.log → .integration-tests/logs/nuxt-ui/snapshot-mismatches.json
 */

import { readFileSync, writeFileSync } from "fs";
import { resolve } from "path";

const logFile =
  process.argv[2] ||
  resolve(".integration-tests/logs/nuxt-ui/verter-test.log");
const outputFile =
  process.argv[3] ||
  resolve(".integration-tests/logs/nuxt-ui/snapshot-mismatches.json");

const raw = readFileSync(logFile, "utf-8");

// Strip ANSI escape codes
const strip = (s) => s.replace(/\x1b\[[0-9;]*m/g, "");

const clean = strip(raw);
const lines = clean.split("\n");

const results = [];
let i = 0;

while (i < lines.length) {
  // Look for: Error: Snapshot `...` mismatched
  const matchError = lines[i].match(
    /^Error: Snapshot `(.+?)` mismatched$/
  );
  if (!matchError) {
    i++;
    continue;
  }

  const snapshotName = matchError[1];

  // Find the FAIL line just above for test file + test name
  let testFile = "";
  let testPath = "";
  for (let j = i - 1; j >= Math.max(0, i - 3); j--) {
    const failMatch = lines[j].match(
      /FAIL\s+(?:nuxt\s+)?(test\/\S+)\s*>\s*(.+)/
    );
    if (failMatch) {
      testFile = failMatch[1].trim();
      testPath = failMatch[2].trim();
      break;
    }
  }

  // Skip to "- Expected" / "+ Received"
  i++;
  while (i < lines.length && !lines[i].startsWith("- Expected")) {
    i++;
  }
  if (i >= lines.length) break;

  // Skip "- Expected", "+ Received", blank line
  i++; // skip "- Expected"
  i++; // skip "+ Received"
  if (i < lines.length && lines[i].trim() === "") i++; // skip blank

  // Now parse diff hunks until we hit the separator (⎯⎯⎯) or file location (❯)
  const expectedLines = [];
  const receivedLines = [];

  while (i < lines.length) {
    const line = lines[i];

    // End markers
    if (line.match(/^⎯⎯/) || line.match(/^❯\s/) || line.match(/^\s*\d+\|/)) {
      break;
    }

    // Hunk header @@ ... @@
    if (line.startsWith("@@")) {
      i++;
      continue;
    }

    // Diff lines
    if (line.startsWith("-")) {
      expectedLines.push(line.slice(1)); // remove leading -
    } else if (line.startsWith("+")) {
      receivedLines.push(line.slice(1)); // remove leading +
    } else if (line.startsWith("  ") || line === "") {
      // Context line (shared between expected and received)
      expectedLines.push(line.startsWith("  ") ? line.slice(2) : line);
      receivedLines.push(line.startsWith("  ") ? line.slice(2) : line);
    }

    i++;
  }

  const expected = expectedLines.join("\n").trim();
  const received = receivedLines.join("\n").trim();

  // Extract just the diff lines (only changed parts, no context)
  const diffExpected = [];
  const diffReceived = [];

  // Re-parse the diff section to get only changed lines
  // We already consumed it, so let's extract from the full expected/received
  // Instead, let's track diffs during parsing. Let me redo with a simpler approach.

  results.push({
    snapshotName,
    testFile,
    testPath,
    expected,
    received,
  });
}

// Post-process: deduplicate and categorize the diffs
// For each mismatch, compute a minimal diff summary
for (const entry of results) {
  const expLines = entry.expected.split("\n");
  const recLines = entry.received.split("\n");

  // Find lines that differ
  const diffs = [];
  const maxLen = Math.max(expLines.length, recLines.length);
  for (let j = 0; j < maxLen; j++) {
    const e = expLines[j] ?? "";
    const r = recLines[j] ?? "";
    if (e !== r) {
      diffs.push({ line: j, expected: e, received: r });
    }
  }
  entry.diffCount = diffs.length;
  entry.diffs = diffs;
}

// Group by component (testFile)
const byComponent = {};
for (const entry of results) {
  const component = entry.testFile || "unknown";
  if (!byComponent[component]) {
    byComponent[component] = [];
  }
  byComponent[component].push(entry);
}

// Analyze common diff patterns
const patternCounts = {};
for (const entry of results) {
  for (const d of entry.diffs) {
    // Normalize: strip dynamic IDs and whitespace to find patterns
    const expNorm = d.expected.replace(/v-\d+-\d+-\d+/g, "v-X").replace(/\s+/g, " ").trim();
    const recNorm = d.received.replace(/v-\d+-\d+-\d+/g, "v-X").replace(/\s+/g, " ").trim();

    // Find what changed between expected and received
    // Simple: check if it's just a class difference, attribute difference, etc.
    if (expNorm === recNorm) continue;

    // Try to extract the actual delta
    // Find longest common prefix and suffix
    let prefixLen = 0;
    while (
      prefixLen < expNorm.length &&
      prefixLen < recNorm.length &&
      expNorm[prefixLen] === recNorm[prefixLen]
    ) {
      prefixLen++;
    }
    let suffixLen = 0;
    while (
      suffixLen < expNorm.length - prefixLen &&
      suffixLen < recNorm.length - prefixLen &&
      expNorm[expNorm.length - 1 - suffixLen] ===
        recNorm[recNorm.length - 1 - suffixLen]
    ) {
      suffixLen++;
    }

    const expDelta = expNorm.slice(prefixLen, expNorm.length - suffixLen);
    const recDelta = recNorm.slice(prefixLen, recNorm.length - suffixLen);

    // Trim to reasonable length for pattern matching
    const key = `${expDelta.slice(0, 100)} → ${recDelta.slice(0, 100)}`;
    patternCounts[key] = (patternCounts[key] || 0) + 1;
  }
}

// Sort patterns by frequency
const sortedPatterns = Object.entries(patternCounts)
  .sort((a, b) => b[1] - a[1])
  .slice(0, 50)
  .map(([pattern, count]) => ({ pattern, count }));

const output = {
  summary: {
    totalMismatches: results.length,
    componentCount: Object.keys(byComponent).length,
    components: Object.fromEntries(
      Object.entries(byComponent).map(([k, v]) => [k, v.length])
    ),
  },
  topDiffPatterns: sortedPatterns,
  mismatches: results,
};

writeFileSync(outputFile, JSON.stringify(output, null, 2), "utf-8");
console.log(`Parsed ${results.length} snapshot mismatches from ${logFile}`);
console.log(`Output: ${outputFile}`);
console.log(`\nTop 10 diff patterns:`);
for (const p of sortedPatterns.slice(0, 10)) {
  console.log(`  [${p.count}x] ${p.pattern}`);
}
console.log(`\nComponents with mismatches:`);
for (const [comp, entries] of Object.entries(byComponent)) {
  console.log(`  ${comp}: ${entries.length}`);
}
