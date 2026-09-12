/**
 * On-demand negative-control replay. Routine CI runs focused fixture tests
 * and the normal test lanes; it does not replay historical evidence records.
 * A replay requires a nonempty, complete clean run, an applicable mutation,
 * and the expected refusal. Passing totals are observations of the current
 * inventory, never values that must match an old transcript. Where the runner
 * can list the selection its command makes, the clean run is held to that
 * listing on the host that runs it, so a green run over fewer cases than the
 * tree currently selects is refused rather than accepted as nonempty work.
 */

import assert from "node:assert/strict";
import fs from "node:fs";
import path from "node:path";
import process from "node:process";

import { parseRefusal, parseTerminalSummary } from "./closure-register.mjs";

/**
 * Output trees, not inputs. A repository mirror copies source and then links
 * each installed `node_modules` tree in place; walking or copying these
 * basenames would multiply the mirror by gigabytes and is never a mutation
 * site.
 */
export const MIRROR_OUTPUT_BASENAMES = new Set([
  ".git",
  "node_modules",
  "target",
  ".integration-tests",
  ".cache",
]);

function existingDirectory(fullPath) {
  try {
    return fs.statSync(fullPath).isDirectory();
  } catch {
    return false;
  }
}

/**
 * Repo-relative installed `node_modules` trees that a cargo record can resolve.
 *
 * The copy filter drops every basename in {@link MIRROR_OUTPUT_BASENAMES}, so a
 * mirror that only junctions the repository ROOT `node_modules` is missing
 * every workspace package's own tree (`packages/typescript-plugin/node_modules`
 * and its siblings). Tests that canonicalize those paths panic in the mirror
 * while passing against the checkout. Discovery stops at each collected tree
 * and never descends into the other output basenames.
 */
export function installedModuleRelatives(repoRoot) {
  const found = [];
  const walk = (dir, relative) => {
    let entries;
    try {
      entries = fs.readdirSync(dir, { withFileTypes: true });
    } catch {
      return;
    }
    for (const entry of entries) {
      const childRel = relative ? `${relative}/${entry.name}` : entry.name;
      const child = path.join(dir, entry.name);
      if (entry.name === "node_modules") {
        if (existingDirectory(child)) found.push(childRel.replaceAll("\\", "/"));
        continue;
      }
      if (MIRROR_OUTPUT_BASENAMES.has(entry.name)) continue;
      if (existingDirectory(child)) walk(child, childRel);
    }
  };
  walk(repoRoot, "");
  return found.sort();
}

/**
 * Junction each installed `node_modules` tree from `repoRoot` into `destRoot`.
 *
 * A junction on Windows needs no privilege and a POSIX symlink needs none
 * either. Nothing a control mutates lives under `node_modules`, so the link is
 * not a mutation copy. An empty directory left behind by a copy filter is
 * replaced; an already-linked dest is left in place.
 */
export function linkInstalledModuleTrees(repoRoot, destRoot) {
  for (const rel of installedModuleRelatives(repoRoot)) {
    const src = path.join(repoRoot, ...rel.split("/"));
    const dest = path.join(destRoot, ...rel.split("/"));
    fs.mkdirSync(path.dirname(dest), { recursive: true });
    if (fs.existsSync(dest)) {
      const st = fs.lstatSync(dest);
      if (st.isSymbolicLink()) continue;
      fs.rmSync(dest, { recursive: true, force: true });
    }
    // `junction` is Windows-only (no privilege). On POSIX the type argument is
    // ignored by Node, but passing it is still a Windows spelling of a POSIX
    // symlink, so the two platforms name the same operation differently.
    try {
      if (process.platform === "win32") fs.symlinkSync(src, dest, "junction");
      else fs.symlinkSync(src, dest);
    } catch (error) {
      throw new Error(`failed to link ${rel} into the control-lane mirror: ${error.message}`);
    }
  }
}

/**
 * Give every file of a freshly copied mirror a modification time of NOW.
 *
 * The replay's cargo target directory persists across runs, and cargo decides
 * freshness by comparing each source file's mtime against the artifact it last
 * built from it. A mutated run builds an artifact from the planted source, the
 * plant is restored, and the next run recreates the mirror from a copy — a
 * copy that, on Windows, keeps the checkout's older timestamps (`fs.cpSync`
 * goes through `CopyFile`, which preserves them). To cargo the artifact built
 * from the MUTATED source is then newer than every source it depends on, so it
 * is reused, and the clean run fails on the previous run's mutation: a refusal
 * no edit in this run produced, on a tree that is not red. A mirror younger
 * than anything the target directory holds makes those fingerprints decide
 * from the bytes actually on disk, on every platform. Linked dependency trees
 * are left alone: nothing is built from them and they are not this mirror's
 * copy.
 */
export function freshenMirrorTimestamps(root) {
  const now = new Date();
  const walk = (dir) => {
    for (const entry of fs.readdirSync(dir, { withFileTypes: true })) {
      if (entry.isSymbolicLink()) continue;
      const child = path.join(dir, entry.name);
      if (entry.isDirectory()) {
        if (!MIRROR_OUTPUT_BASENAMES.has(entry.name)) walk(child);
        continue;
      }
      fs.utimesSync(child, now, now);
    }
  };
  walk(root);
}

/** Entry point for the manually dispatched diagnostic workflow. */
export const CONTROL_LANE_ENTRY = "roadmap/0.1.0-tama/tools/closure-controls.test.mjs";
export const CONTROL_LANE_DEADLINE_MS = 40 * 60_000;
export const CONTROL_LANE_COMMAND_DEADLINE_MS = 20 * 60_000;

/**
 * The preload that turns a node:test suite file into its own inventory,
 * spelled relative to the repository root every replay runs its commands from.
 * It is spawned, never imported: importing it would replace `node:test` in
 * the importing process too.
 */
export const NODE_TEST_INVENTORY_PRELOAD = "./roadmap/0.1.0-tama/tools/node-test-inventory.mjs";

/**
 * The single-verdict tools whose counted field is a selection the tree
 * declares independently of the tool, keyed by the tool a record runs, with
 * the key that count is printed under and the script that re-derives it.
 * A tool absent here has no inventory, and its clean run is judged on its own
 * line alone.
 */
export const TOOL_LINE_INVENTORIES = Object.freeze({
  "roadmap/0.1.0-tama/tools/validate-program-dag.mjs": Object.freeze({
    countKey: "nodes",
    script: "./roadmap/0.1.0-tama/tools/dag-node-inventory.mjs",
  }),
});

/**
 * The command that LISTS the selection a run command would execute, or `null`
 * for a runner that has none.
 *
 * The listing is produced by the same runner over the same selection arguments
 * as the clean run, so the run is held to what the tree selects today rather
 * than to a transcribed total. `cargo nextest list` reports, per case, whether
 * the run's filter matches it — exactly the split the run summary reports as
 * executed and skipped. libtest's `--list` prints every case the filter admits,
 * ignored ones included; the ignored subset is listed on request with
 * `--ignored`. node:test has no listing mode, so its inventory is the suite's
 * own module code run under {@link NODE_TEST_INVENTORY_PRELOAD}, which counts
 * every case the suite declares and runs none of them. A single-verdict tool
 * lists nothing either; where its count is a selection the tree declares,
 * {@link TOOL_LINE_INVENTORIES} names the script that re-derives it.
 */
export function inventoryCommand(adapter, argv, { ignoredOnly = false } = {}) {
  if (adapter.summary_grammar === "tool-line") {
    const inventory = TOOL_LINE_INVENTORIES[argv[0]];
    return inventory ? [inventory.script] : null;
  }
  if (adapter.summary_grammar === "node-test") {
    // The run is `--test <suite>`; the inventory is that same suite file
    // evaluated with `node:test` replaced by the counting registrar. Node
    // evaluates one main module, so a record selecting several files, or
    // narrowing a file by flag, has no inventory this can derive and is
    // refused rather than held to a count of something else.
    const [flag, ...files] = argv;
    assert.ok(
      flag === "--test" && files.length === 1 && !files[0].startsWith("-"),
      `a node-test record must run exactly one whole suite file to be inventoried, got ${JSON.stringify(argv)}`,
    );
    return ["--import", NODE_TEST_INVENTORY_PRELOAD, files[0]];
  }
  if (adapter.summary_grammar === "nextest") {
    // The listing flag is a `list` option, so it precedes any `--` the tail
    // carries.
    assert.deepEqual(
      argv.slice(0, 2),
      ["nextest", "run"],
      `a nextest record's command must start with \`nextest run\`, got ${JSON.stringify(argv)}`,
    );
    return ["nextest", "list", "--message-format", "json", ...argv.slice(2)];
  }
  if (adapter.summary_grammar === "libtest") {
    // libtest flags follow the `--` separator. A tail that already carries one
    // (`-- --exact …`) takes `--list` after its own filters, so the listing is
    // exactly the run's selection.
    const listed = argv.includes("--") ? [...argv, "--list"] : [...argv, "--", "--list"];
    return ignoredOnly ? [...listed, "--ignored"] : listed;
  }
  return null;
}

/**
 * The selection a runner's own listing reports, or `null` when the output is
 * not a listing of the declared shape.
 */
export function parseInventory(grammar, output) {
  if (grammar === "nextest") {
    let listing;
    try {
      listing = JSON.parse(output);
    } catch {
      return null;
    }
    const suites = listing?.["rust-suites"];
    if (!suites || typeof suites !== "object") return null;
    let executed = 0;
    let skipped = 0;
    for (const suite of Object.values(suites))
      for (const testcase of Object.values(suite?.testcases ?? {}))
        if (testcase?.["filter-match"]?.status === "matches") executed += 1;
        else skipped += 1;
    return { selected: executed + skipped, executed, skipped };
  }
  if (grammar === "libtest") {
    const text = output.replaceAll("\r\n", "\n");
    // Every binary the command selects prints its cases and then its own
    // `N tests, M benchmarks` line; a listing with no such line is not a
    // listing, and one whose per-binary totals disagree with the cases it
    // printed is a truncated one.
    const totals = [...text.matchAll(/^(\d+) tests?, (\d+) benchmarks?$/gmu)];
    if (!totals.length) return null;
    const listed = [...text.matchAll(/^\S.*: test$/gmu)].length;
    const selected = totals.reduce((sum, row) => sum + Number(row[1]), 0);
    if (listed !== selected) return null;
    return { selected };
  }
  if (grammar === "node-test") {
    // The preload's single exit line. Two such lines mean two processes'
    // counts were interleaved into one listing.
    const rows = [
      ...output
        .replaceAll("\r\n", "\n")
        .matchAll(/^node-test inventory: declared (\d+) skipped (\d+)$/gmu),
    ];
    if (rows.length !== 1) return null;
    return { selected: Number(rows[0][1]), skipped: Number(rows[0][2]) };
  }
  if (grammar === "tool-line") {
    // The inventory script's one line, naming the key its count is under so it
    // is compared only against the field the record counts.
    const rows = [
      ...output.replaceAll("\r\n", "\n").matchAll(/^tool-line inventory: ([a-z_]+)=(\d+)$/gmu),
    ];
    if (rows.length !== 1) return null;
    return { countKey: rows[0][1], selected: Number(rows[0][2]) };
  }
  return null;
}

/**
 * The case names a libtest command selects by exact name, or `null` when it
 * selects by filter. An exact selection states the intended work in the command
 * itself, so a named case that no longer exists leaves a green run over fewer
 * cases than the record intends.
 */
export function exactSelection(argv) {
  const separator = argv.indexOf("--");
  if (separator === -1) return null;
  const tail = argv.slice(separator + 1);
  if (!tail.includes("--exact")) return null;
  return tail.filter((token) => !token.startsWith("-"));
}

// The clean run's selection, held to the tree's own inventory of it. Totals are
// compared against the listing produced now, never against a transcript.
function assertCleanRunSelection({ control, proof, adapter, argv, observedNow, inventory }) {
  const command = argv.join(" ");
  const grammar = adapter.summary_grammar;
  const exact = grammar === "libtest" ? exactSelection(argv) : null;
  if (exact)
    assert.equal(
      observedNow.executed,
      exact.length,
      `${control.id}: ${command} names ${exact.length} cases with --exact, but the clean run executed ${observedNow.executed}, so a case the record intends to run no longer exists on this tree`,
    );
  if (inventory === null) return;
  if (inventory.countKey !== undefined)
    assert.equal(
      inventory.countKey,
      proof.count_key,
      `${control.id}: record ${proof.id} counts ${proof.count_key}, but the inventory of ${command} counts ${inventory.countKey}, so the two cannot be compared`,
    );
  assert.equal(
    observedNow.selected,
    inventory.selected,
    `${control.id}: the clean run of ${command} selected ${observedNow.selected} cases, but the runner's own listing of that selection on this tree holds ${inventory.selected}`,
  );
  if (inventory.skipped !== undefined)
    assert.equal(
      observedNow.skipped,
      inventory.skipped,
      `${control.id}: the clean run of ${command} skipped ${observedNow.skipped} cases, but this tree marks ${inventory.skipped} of that selection ignored, so the difference is an unexpected skip`,
    );
}

// Runs the runner's own listing of the clean run's selection, or returns null
// for a runner that has none. A listing is read from STDOUT alone: cargo prints
// build progress on stderr, and a JSON listing with `Compiling` lines appended
// is not JSON. The whole output still goes into a failure message.
function listSelection({ control, adapter, argv, observedNow, spawn }) {
  const listing = inventoryCommand(adapter, argv);
  if (!listing) return null;
  const listed = spawn(listing, adapter);
  const listedOutput = `${listed.stdout ?? ""}${listed.stderr ?? ""}`;
  assert.equal(
    listed.status,
    0,
    `${control.id}: the runner refused to list the selection of ${argv.join(" ")}, so the clean run cannot be held to the tree's own inventory\n${listedOutput}`,
  );
  let inventory = parseInventory(adapter.summary_grammar, listed.stdout ?? "");
  assert.ok(
    inventory,
    `${control.id}: ${listing.join(" ")} emitted no ${adapter.summary_grammar} listing:\n${listedOutput}`,
  );
  // libtest lists ignored cases beside the runnable ones without marking them,
  // so a clean run that reports ignored cases gets that subset listed on its
  // own.
  if (adapter.summary_grammar === "libtest" && observedNow.skipped > 0) {
    const ignoredListing = inventoryCommand(adapter, argv, { ignoredOnly: true });
    const ignored = spawn(ignoredListing, adapter);
    const ignoredOutput = `${ignored.stdout ?? ""}${ignored.stderr ?? ""}`;
    assert.equal(
      ignored.status,
      0,
      `${control.id}: the runner refused to list the ignored subset of ${argv.join(" ")}\n${ignoredOutput}`,
    );
    const ignoredInventory = parseInventory(adapter.summary_grammar, ignored.stdout ?? "");
    assert.ok(
      ignoredInventory,
      `${control.id}: ${ignoredListing.join(" ")} emitted no ${adapter.summary_grammar} listing:\n${ignoredOutput}`,
    );
    inventory = { ...inventory, skipped: ignoredInventory.selected };
  }
  return inventory;
}

/**
 * The command a control's bound record runs, with the adapter's prefix applied.
 *
 * The binding is derived from the register rather than volunteered: a control
 * no record names has no command to re-run, and that is an error rather than an
 * exemption.
 */
export function controlCommand(model, control) {
  const adapters = new Map(model.register.adapter.map((row) => [row.id, row]));
  const proof = model.register.proof.find((row) => row.control === control.id);
  assert.ok(proof, `control ${control.id} is bound to no record`);
  const adapter = adapters.get(proof.adapter);
  assert.ok(adapter, `record ${proof.id} names unknown adapter ${proof.adapter}`);
  return { proof, adapter, argv: [...adapter.argv_prefix, ...proof.argv_tail] };
}

/**
 * Replay a control in a disposable mirror. The caller supplies the runner;
 * this function owns clean-run validation, planting, restoration, and refusal
 * comparison. The recorded command still selects the work, but unrelated
 * passing, skipped, or fixture inventory changes do not invalidate a replay.
 */
export function reapply({ model, control, mirror, spawn }) {
  const { proof, adapter, argv } = controlCommand(model, control);
  const count = (haystack, needle) => haystack.split(needle).length - 1;

  // A source control edits one file of the mirror. Its bytes are normalized to
  // LF before anything is counted or written: the validator normalizes the
  // same file before its unique-occurrence checks, so a checkout whose
  // subjects carry CRLF would pass `--check` while this routine, reading raw
  // bytes, found zero occurrences of a recipe recorded with LF separators and
  // failed a uniqueness the validator had already established. The normalized
  // text is also the base for the plant, so what runs is what was checked; the
  // original bytes are what get restored.
  const subject = control.kind === "source" ? path.join(mirror, control.subject) : null;
  const original = subject === null ? null : fs.readFileSync(subject, "utf8");
  const text = original === null ? null : original.replaceAll("\r\n", "\n");

  // The mutation must be provably applicable and provably absent before it is
  // applied: a recipe matching nothing, or one already in the tree, would
  // otherwise produce a refusal that says nothing about the mutation. For a
  // command mutation that is a statement over the argument vector — every
  // added argument must be new to the command it extends.
  if (control.kind === "source") {
    assert.equal(
      count(text, control.reverted),
      1,
      `${control.id}: the text this mutation replaces does not occur exactly once in ${control.subject}`,
    );
    assert.equal(
      count(text, control.applied),
      0,
      `${control.id}: the mutation is already present in ${control.subject}`,
    );
  } else {
    assert.ok(
      control.argv_delta?.length,
      `${control.id}: a command mutation names no arguments to add`,
    );
    for (const token of control.argv_delta)
      assert.ok(
        !argv.includes(token),
        `${control.id}: ${JSON.stringify(token)} is already part of the command this mutation claims to extend`,
      );
  }

  // The clean run BEFORE the mutation is what makes the refusal attributable.
  // Without it a mirror that was already red would report every control as
  // killed, which is the same false pass as a mutation that never applied.
  const clean = spawn(argv, adapter);
  const cleanOutput = `${clean.stdout ?? ""}${clean.stderr ?? ""}`.replaceAll("\r\n", "\n");
  assert.equal(
    clean.status,
    0,
    `${control.id}: the mirror refuses ${argv.join(" ")} before the mutation, so a refusal after it would prove nothing\n${cleanOutput}`,
  );

  const observedNow = parseTerminalSummary(adapter.summary_grammar, cleanOutput, proof.count_key);
  assert.ok(
    observedNow,
    `${control.id}: the clean run of ${argv.join(" ")} emitted no ${adapter.summary_grammar} summary:\n${cleanOutput}`,
  );
  assert.ok(
    Object.values(observedNow).every((value) => Number.isSafeInteger(value) && value >= 0) &&
      observedNow.executed > 0 &&
      observedNow.failed === 0 &&
      observedNow.passed === observedNow.executed &&
      observedNow.selected === observedNow.executed + observedNow.skipped,
    `${control.id}: the clean run must execute nonempty work and pass completely:\n${cleanOutput}`,
  );
  const inventory = listSelection({ control, adapter, argv, observedNow, spawn });
  assertCleanRunSelection({ control, proof, adapter, argv, observedNow, inventory });

  let mutated;
  if (control.kind === "source") {
    try {
      // A function replacement, not a string one: `String.prototype.replace`
      // reads `$&`, `$1`, `$'`, "$`" and `$$` out of a string replacement, so a
      // control mutating any line carrying `${{ ... }}` — every workflow line
      // does — would write bytes that differ from the ones the register records,
      // and the refusal would then be attributed to a mutation that never
      // landed.
      fs.writeFileSync(
        subject,
        text.replace(control.reverted, () => control.applied),
      );
      mutated = spawn(argv, adapter);
    } finally {
      // A mutation belongs in a copy, never in the tree under review, where an
      // interrupted run would leave it behind as a real edit.
      fs.writeFileSync(subject, original);
    }
  } else {
    mutated = spawn([...argv, ...control.argv_delta], adapter);
  }

  const output = `${mutated.stdout ?? ""}${mutated.stderr ?? ""}`.replaceAll("\r\n", "\n");
  assert.ok(
    Number.isInteger(mutated.status) && mutated.status > 0 && !mutated.signal && !mutated.error,
    `${control.id}: the mutated command must terminate with a nonzero exit, without a signal or spawn error\n${output}`,
  );
  const live = parseRefusal(adapter.summary_grammar, output, proof.count_key);
  assert.ok(
    live,
    `${control.id}: the mutated run emitted no ${adapter.summary_grammar} refusal:\n${output}`,
  );
  assert.deepEqual(
    refusalReason(live),
    refusalReason(parseRefusal(adapter.summary_grammar, control.observed, proof.count_key)),
    `${control.id}: the mutated command produced a different refusal:\n${output}`,
  );
  return { proof, adapter, argv, output, cleanSummary: observedNow };
}

// Keep the refusal kind, failed count, and tool diagnostics. Total inventory
// and successful siblings do not establish whether a mutation was detected.
function refusalReason(refusal) {
  if (!refusal) return null;
  const inventory = new Set([
    "selected",
    "executed",
    "passed",
    "skipped",
    "ignored",
    "binaries",
    "fixtures",
    "cleared",
  ]);
  return Object.fromEntries(Object.entries(refusal).filter(([key]) => !inventory.has(key)));
}
