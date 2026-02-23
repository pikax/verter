/**
 * Edit-Ops vs Full-String Benchmark
 *
 * Compares two approaches for returning compiled output from Rust to JS:
 *   A) Current: Rust returns the full compiled string via getVirtualFile
 *   B) Proposed: Rust returns compact edit operations + JS applies them with MagicString
 *
 * Run: pnpm --filter @verter/benchmark run bench:edit-ops
 */
import { Bench } from "tinybench";
import { readFileSync } from "fs";
import { join, dirname } from "path";
import { fileURLToPath } from "url";
import MagicString from "magic-string";
import { VerterHost } from "@verter/native";

const __filename = fileURLToPath(import.meta.url);
const __dirname = dirname(__filename);

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/** An edit operation: either keep a range from the original, or insert new content. */
type EditOp =
  | { type: "keep"; start: number; end: number }
  | { type: "insert"; content: string };

interface Fixture {
  name: string;
  source: string;
  buffer: Buffer;
  size: number;
}

interface FixtureAnalysis {
  fixture: Fixture;
  compiledCode: string;
  editOps: EditOp[];
  /** Bytes of generated content (inserts only, not kept ranges) */
  editOpsPayloadSize: number;
  /** Approximate bytes to serialize the full edit-ops array */
  editOpsWireSize: number;
}

// ---------------------------------------------------------------------------
// Fixtures
// ---------------------------------------------------------------------------

const FIXTURE_FILES = [
  "tiny-template.vue",
  "simple-interactive.vue",
  "list-rendering.vue",
  "conditional-heavy.vue",
  "form-component.vue",
  "composition-heavy.vue",
  "template-heavy.vue",
  "kitchen-sink.vue",
];

function loadFixtures(): Fixture[] {
  const dir = join(__dirname, "fixtures");
  return FIXTURE_FILES.map((f) => {
    const source = readFileSync(join(dir, f), "utf-8");
    return {
      name: f.replace(".vue", ""),
      source,
      buffer: Buffer.from(source),
      size: Buffer.byteLength(source),
    };
  });
}

// ---------------------------------------------------------------------------
// Edit-ops computation
// ---------------------------------------------------------------------------

/**
 * Find all occurrences of `needle` in `haystack` and return their start offsets.
 */
function findAll(haystack: string, needle: string): number[] {
  const results: number[] = [];
  let pos = 0;
  while (pos <= haystack.length - needle.length) {
    const idx = haystack.indexOf(needle, pos);
    if (idx === -1) break;
    results.push(idx);
    pos = idx + 1;
  }
  return results;
}

/**
 * Compute edit operations that transform the original source into the compiled output.
 *
 * Strategy: greedily find the longest matching substrings between original and compiled,
 * building a sequence of keep/insert operations. This simulates what Verter would return
 * if it tracked which parts of the output came verbatim from the source.
 *
 * In practice, Verter already knows this — the script body is copied verbatim, and
 * everything else (imports, wrapper, render function) is generated.
 */
function computeEditOps(original: string, compiled: string): EditOp[] {
  // Find contiguous chunks of the original that appear in the compiled output.
  // We use a greedy approach: scan compiled output, find longest match in original.
  const MIN_MATCH_LEN = 20; // Don't bother with tiny matches
  const ops: EditOp[] = [];

  // Build a map of substrings from original for fast lookup
  // For efficiency, we look for line-based matches first
  const originalLines = original.split("\n");
  const lineStarts: Map<string, number[]> = new Map();

  let offset = 0;
  for (const line of originalLines) {
    const trimmed = line.trimStart();
    if (trimmed.length >= MIN_MATCH_LEN) {
      const key = trimmed.substring(0, Math.min(20, trimmed.length));
      if (!lineStarts.has(key)) lineStarts.set(key, []);
      lineStarts.get(key)!.push(offset);
    }
    offset += line.length + 1; // +1 for \n
  }

  // Find the longest contiguous match starting from each position in compiled
  function findLongestMatch(
    compiledPos: number,
  ): { origStart: number; origEnd: number; compiledEnd: number } | null {
    let bestMatch: {
      origStart: number;
      origEnd: number;
      compiledEnd: number;
    } | null = null;

    // Try to match from current compiled position against original
    const compiledSlice = compiled.substring(
      compiledPos,
      compiledPos + Math.min(20, compiled.length - compiledPos),
    );
    if (compiledSlice.length < MIN_MATCH_LEN) return null;

    const key = compiledSlice
      .trimStart()
      .substring(0, Math.min(20, compiledSlice.trimStart().length));

    // Check line-based matches
    const candidates = lineStarts.get(key) || [];

    // Also do a direct search for the first 20 chars
    const directMatches = findAll(original, compiledSlice.substring(0, 20));

    const allCandidates = [...candidates, ...directMatches];

    for (const origPos of allCandidates) {
      // Extend the match as far as possible
      let matchLen = 0;
      const maxLen = Math.min(
        original.length - origPos,
        compiled.length - compiledPos,
      );
      while (
        matchLen < maxLen &&
        original[origPos + matchLen] === compiled[compiledPos + matchLen]
      ) {
        matchLen++;
      }

      if (
        matchLen >= MIN_MATCH_LEN &&
        (!bestMatch ||
          matchLen > bestMatch.origEnd - bestMatch.origStart)
      ) {
        bestMatch = {
          origStart: origPos,
          origEnd: origPos + matchLen,
          compiledEnd: compiledPos + matchLen,
        };
      }
    }

    return bestMatch;
  }

  // Scan through compiled output
  let compiledPos = 0;
  while (compiledPos < compiled.length) {
    const match = findLongestMatch(compiledPos);

    if (match && match.origEnd - match.origStart >= MIN_MATCH_LEN) {
      // Insert any content before the match
      if (compiledPos < match.compiledEnd - (match.origEnd - match.origStart)) {
        const insertContent = compiled.substring(
          compiledPos,
          match.compiledEnd - (match.origEnd - match.origStart),
        );
        if (insertContent.length > 0) {
          ops.push({ type: "insert", content: insertContent });
        }
      }
      // Keep from original
      ops.push({ type: "keep", start: match.origStart, end: match.origEnd });
      compiledPos = match.compiledEnd;
    } else {
      // No match found — scan forward to find next match
      let nextMatchPos = compiledPos + 1;
      let nextMatch = null;
      while (nextMatchPos < compiled.length && nextMatchPos < compiledPos + 500) {
        nextMatch = findLongestMatch(nextMatchPos);
        if (nextMatch) break;
        nextMatchPos++;
      }

      if (nextMatch) {
        // Insert everything up to the next match
        const insertContent = compiled.substring(
          compiledPos,
          nextMatchPos,
        );
        if (insertContent.length > 0) {
          ops.push({ type: "insert", content: insertContent });
        }
        compiledPos = nextMatchPos;
      } else {
        // No more matches — insert everything remaining
        ops.push({
          type: "insert",
          content: compiled.substring(compiledPos),
        });
        break;
      }
    }
  }

  return ops;
}

/**
 * Apply edit operations to reconstruct the compiled output using MagicString.
 * This is the JS-side cost of the proposed approach.
 */
function applyEditOps(original: string, ops: EditOp[]): string {
  // The edit-ops approach would work differently from MagicString.overwrite on original.
  // Instead, we'd concatenate: for each op, either copy from original or insert new content.
  // This is closer to how a real implementation would work.
  let result = "";
  for (const op of ops) {
    if (op.type === "keep") {
      result += original.substring(op.start, op.end);
    } else {
      result += op.content;
    }
  }
  return result;
}

/**
 * Apply edit operations using MagicString (more realistic — preserves sourcemaps).
 * Builds a new MagicString by composing chunks.
 */
function applyEditOpsWithMagicString(
  _original: string,
  ops: EditOp[],
): string {
  // MagicString doesn't directly support "build from ops" — it patches an existing string.
  // For a fair comparison, simulate by building the output with string concatenation
  // (which is what MagicString.toString() effectively does internally for sourcemaps).
  //
  // In a real implementation, MagicString would be used on the *original* source
  // to overwrite/remove/append sections. Let's benchmark that approach too.
  const parts: string[] = [];
  for (const op of ops) {
    if (op.type === "keep") {
      parts.push(_original.substring(op.start, op.end));
    } else {
      parts.push(op.content);
    }
  }
  return parts.join("");
}

/**
 * Simulate the MagicString-on-original approach: start with the original source
 * as a MagicString, then overwrite/remove/prepend/append to produce the compiled output.
 * This is the most realistic "edit ops" implementation.
 */
function applyWithMagicStringOnOriginal(
  original: string,
  compiled: string,
  ops: EditOp[],
): string {
  // Build a MagicString from a buffer large enough to hold the output.
  // We prepend the generated header, keep original ranges, and append the rest.
  const s = new MagicString(original);

  // First, figure out which ranges of the original are kept and which are replaced.
  // The ops tell us which original ranges appear in the output.
  const keptRanges: Array<{ start: number; end: number }> = [];
  for (const op of ops) {
    if (op.type === "keep") {
      keptRanges.push({ start: op.start, end: op.end });
    }
  }

  // Sort kept ranges by start position
  keptRanges.sort((a, b) => a.start - b.start);

  // Remove everything NOT in kept ranges
  let pos = 0;
  for (const range of keptRanges) {
    if (pos < range.start) {
      s.remove(pos, range.start);
    }
    pos = range.end;
  }
  if (pos < original.length) {
    s.remove(pos, original.length);
  }

  // Now prepend the generated header and append the generated footer
  // (We can't easily insert between kept ranges with MagicString,
  //  so this is an approximation)
  let headerContent = "";
  let footerContent = "";
  let seenKeep = false;
  for (const op of ops) {
    if (op.type === "insert") {
      if (!seenKeep) {
        headerContent += op.content;
      } else {
        footerContent += op.content;
      }
    } else {
      seenKeep = true;
    }
  }

  if (headerContent) s.prepend(headerContent);
  if (footerContent) s.append(footerContent);

  return s.toString();
}

// ---------------------------------------------------------------------------
// Analysis
// ---------------------------------------------------------------------------

function analyzeFixture(host: VerterHost, fixture: Fixture): FixtureAnalysis {
  host.upsert({ inputId: `${fixture.name}.vue`, source: fixture.buffer });
  const vf = host.getVirtualFile({
    canonicalId: `${fixture.name}.vue`,
    nodeKind: { kind: "main" },
  });

  const editOps = computeEditOps(fixture.source, vf.code);

  // Calculate payload size (only insert content — keep ops are just offsets)
  let insertContentSize = 0;
  for (const op of editOps) {
    if (op.type === "insert") {
      insertContentSize += op.content.length;
    }
  }

  // Wire size: each keep op = ~12 bytes (type + start + end), each insert = content + ~8 bytes overhead
  const wireSize =
    editOps.reduce((sum, op) => {
      if (op.type === "keep") return sum + 12;
      return sum + op.content.length + 8;
    }, 0) + 16; // array overhead

  return {
    fixture,
    compiledCode: vf.code,
    editOps,
    editOpsPayloadSize: insertContentSize,
    editOpsWireSize: wireSize,
  };
}

// ---------------------------------------------------------------------------
// Printing helpers
// ---------------------------------------------------------------------------

function pad(s: string, n: number): string {
  return s.padStart(n);
}

function fmtNs(ns: number): string {
  if (ns < 1_000) return `${ns.toFixed(0)} ns`;
  if (ns < 1_000_000) return `${(ns / 1_000).toFixed(2)} µs`;
  return `${(ns / 1_000_000).toFixed(3)} ms`;
}

function fmtBytes(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`;
  return `${(bytes / 1024).toFixed(2)} KB`;
}

function printBenchResults(bench: Bench) {
  for (const task of bench.tasks) {
    const r = task.result!;
    const meanNs = r.mean * 1_000_000; // ms → ns
    console.log(
      `    ${task.name.padEnd(50)} ${pad(fmtNs(meanNs), 12)}  (${pad(r.hz.toFixed(0), 8)} ops/s)`,
    );
  }
}

function printHeader(title: string) {
  console.log("");
  console.log("━".repeat(82));
  console.log(`  ${title}`);
  console.log("━".repeat(82));
}

function printSubHeader(title: string) {
  console.log(`\n── ${title} ──`);
}

// ---------------------------------------------------------------------------
// Benchmarks
// ---------------------------------------------------------------------------

async function benchEditOpsVsFullString(analyses: FixtureAnalysis[]) {
  printHeader("EDIT-OPS vs FULL-STRING — DATA SIZE COMPARISON");
  console.log(
    "  Compares data transferred: full compiled string vs edit-ops payload",
  );

  console.log(
    `\n    ${"Fixture".padEnd(25)} ${"Source".padStart(10)} ${"Compiled".padStart(10)} ${"Edit Payload".padStart(13)} ${"Edit Wire".padStart(11)} ${"Savings".padStart(9)}`,
  );
  console.log("    " + "─".repeat(78));

  for (const a of analyses) {
    const savings = (
      (1 - a.editOpsWireSize / a.compiledCode.length) *
      100
    ).toFixed(0);
    const keepOps = a.editOps.filter((o) => o.type === "keep").length;
    const insertOps = a.editOps.filter((o) => o.type === "insert").length;
    console.log(
      `    ${a.fixture.name.padEnd(25)} ${pad(fmtBytes(a.fixture.size), 10)} ${pad(fmtBytes(a.compiledCode.length), 10)} ${pad(fmtBytes(a.editOpsPayloadSize), 13)} ${pad(fmtBytes(a.editOpsWireSize), 11)} ${pad(savings + "%", 9)}  (${keepOps} keep, ${insertOps} insert)`,
    );
  }
}

async function benchJsSideApplicationCost(analyses: FixtureAnalysis[]) {
  printHeader("EDIT-OPS — JS-SIDE APPLICATION COST");
  console.log(
    "  Measures how long JS takes to reconstruct the output from edit-ops",
  );

  for (const a of analyses) {
    printSubHeader(
      `${a.fixture.name} (${fmtBytes(a.fixture.size)} → ${fmtBytes(a.compiledCode.length)})`,
    );

    const bench = new Bench({ time: 2000, warmupIterations: 50 });

    // String concatenation (simplest)
    bench.add("concat: apply edit-ops", () => {
      applyEditOps(a.fixture.source, a.editOps);
    });

    // Array.join approach
    bench.add("join: apply edit-ops", () => {
      applyEditOpsWithMagicString(a.fixture.source, a.editOps);
    });

    // MagicString on original (most realistic for sourcemaps)
    bench.add("MagicString: prepend/remove/append", () => {
      applyWithMagicStringOnOriginal(
        a.fixture.source,
        a.compiledCode,
        a.editOps,
      );
    });

    // Baseline: just receiving a string (simulates getVirtualFile JS overhead)
    bench.add("baseline: string copy", () => {
      // Simulate receiving the compiled string (V8 already allocated it)
      const _copy = a.compiledCode.slice(0);
      void _copy;
    });

    await bench.run();
    printBenchResults(bench);
  }
}

async function benchEndToEnd(analyses: FixtureAnalysis[], host: VerterHost) {
  printHeader("END-TO-END — getVirtualFile vs EDIT-OPS TOTAL");
  console.log(
    "  Current: getVirtualFile returns full string",
  );
  console.log(
    "  Alternative: getVirtualFile returns edit-ops + JS applies them",
  );

  for (const a of analyses) {
    printSubHeader(
      `${a.fixture.name} (${fmtBytes(a.fixture.size)} → ${fmtBytes(a.compiledCode.length)})`,
    );

    const bench = new Bench({ time: 2000, warmupIterations: 50 });

    // Current approach: full getVirtualFile
    bench.add("current: getVirtualFile (full string)", () => {
      host.getVirtualFile({
        canonicalId: `${a.fixture.name}.vue`,
        nodeKind: { kind: "main" },
      });
    });

    // Proposed approach: getVirtualFile + edit-ops application
    // We simulate the transfer cost by calling getVirtualFile (which dominates)
    // and then applying edit-ops in JS
    bench.add("proposed: getVirtualFile + edit-ops apply", () => {
      // In reality, this would call a hypothetical getVirtualEdits() that's cheaper
      // For now, we measure the JS-side overhead that would be ADDED
      host.getVirtualFile({
        canonicalId: `${a.fixture.name}.vue`,
        nodeKind: { kind: "main" },
      });
      applyEditOps(a.fixture.source, a.editOps);
    });

    // Pure edit-ops application (this is the JS-side overhead)
    bench.add("edit-ops apply only (JS overhead)", () => {
      applyEditOps(a.fixture.source, a.editOps);
    });

    await bench.run();
    printBenchResults(bench);
  }
}

async function benchNapiTransferCost(host: VerterHost, analyses: FixtureAnalysis[]) {
  printHeader("NAPI TRANSFER COST — STRING SIZE vs LATENCY");
  console.log(
    "  Measures how getVirtualFile latency correlates with output size",
  );
  console.log(
    "  This reveals per-byte NAPI transfer overhead",
  );

  const bench = new Bench({ time: 2000, warmupIterations: 30 });

  for (const a of analyses) {
    bench.add(
      `getVirtualFile: ${a.fixture.name} (${fmtBytes(a.compiledCode.length)})`,
      () => {
        host.getVirtualFile({
          canonicalId: `${a.fixture.name}.vue`,
          nodeKind: { kind: "main" },
        });
      },
    );
  }

  await bench.run();

  console.log(
    `\n    ${"Fixture".padEnd(35)} ${"Output Size".padStart(12)} ${"Latency".padStart(12)} ${"ns/byte".padStart(10)}`,
  );
  console.log("    " + "─".repeat(69));

  for (const task of bench.tasks) {
    const r = task.result!;
    const meanNs = r.mean * 1_000_000;
    const a = analyses.find((x) => task.name.includes(x.fixture.name))!;
    const nsPerByte = meanNs / a.compiledCode.length;
    console.log(
      `    ${task.name.padEnd(35)} ${pad(fmtBytes(a.compiledCode.length), 12)} ${pad(fmtNs(meanNs), 12)} ${pad(nsPerByte.toFixed(2), 10)}`,
    );
  }

  // Extrapolate: what would edit-ops transfer cost be?
  console.log("\n  Extrapolated edit-ops transfer cost (based on per-byte NAPI overhead):");
  console.log(
    `    ${"Fixture".padEnd(25)} ${"Full String".padStart(12)} ${"Edit Wire".padStart(12)} ${"Est. Savings".padStart(14)}`,
  );
  console.log("    " + "─".repeat(63));

  // Use average ns/byte from results
  const avgNsPerByte =
    bench.tasks.reduce((sum, task) => {
      const r = task.result!;
      const meanNs = r.mean * 1_000_000;
      const a = analyses.find((x) => task.name.includes(x.fixture.name))!;
      return sum + meanNs / a.compiledCode.length;
    }, 0) / bench.tasks.length;

  for (const a of analyses) {
    const fullCostNs = a.compiledCode.length * avgNsPerByte;
    const editCostNs = a.editOpsWireSize * avgNsPerByte;
    const savingsNs = fullCostNs - editCostNs;
    console.log(
      `    ${a.fixture.name.padEnd(25)} ${pad(fmtNs(fullCostNs), 12)} ${pad(fmtNs(editCostNs), 12)} ${pad(fmtNs(savingsNs), 14)}`,
    );
  }
}

// ---------------------------------------------------------------------------
// Main
// ---------------------------------------------------------------------------

async function main() {
  console.log("━".repeat(82));
  console.log("  EDIT-OPS vs FULL-STRING BENCHMARK");
  console.log("  Is it cheaper to return edit operations + apply in JS?");
  console.log("━".repeat(82));

  const fixtures = loadFixtures();
  const host = new VerterHost({ analysisLevel: "none" });

  // Prime all fixtures
  for (const f of fixtures) {
    host.upsert({ inputId: `${f.name}.vue`, source: f.buffer });
  }

  // Analyze all fixtures
  console.log("\nAnalyzing fixtures...");
  const analyses = fixtures.map((f) => analyzeFixture(host, f));

  // Verify edit-ops produce correct output
  for (const a of analyses) {
    const reconstructed = applyEditOps(a.fixture.source, a.editOps);
    if (reconstructed !== a.compiledCode) {
      console.log(
        `  WARNING: ${a.fixture.name} edit-ops reconstruction mismatch!`,
      );
      console.log(`    Expected length: ${a.compiledCode.length}`);
      console.log(`    Got length: ${reconstructed.length}`);
      // Find first difference
      for (let i = 0; i < Math.max(reconstructed.length, a.compiledCode.length); i++) {
        if (reconstructed[i] !== a.compiledCode[i]) {
          console.log(`    First diff at position ${i}:`);
          console.log(`      Expected: ${JSON.stringify(a.compiledCode.substring(i, i + 50))}`);
          console.log(`      Got:      ${JSON.stringify(reconstructed.substring(i, i + 50))}`);
          break;
        }
      }
    } else {
      console.log(`  ✓ ${a.fixture.name}: edit-ops reconstruction verified`);
    }
  }

  await benchEditOpsVsFullString(analyses);
  await benchNapiTransferCost(host, analyses);
  await benchJsSideApplicationCost(analyses);
  await benchEndToEnd(analyses, host);

  console.log("\n" + "━".repeat(82));
  console.log("  DONE");
  console.log("━".repeat(82));

  process.exit(0);
}

main().catch((error) => {
  console.error("Benchmark failed:", error);
  process.exit(1);
});
