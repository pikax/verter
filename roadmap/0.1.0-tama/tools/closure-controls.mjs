/**
 * The negative-control driver.
 *
 * A control's transcript describes a run somebody once did. The uniqueness and
 * absence checks the validator applies beside it establish that the mutation
 * COULD have applied to this tree and is not sitting in it — but nothing there
 * re-runs anything, so a control whose refusal has since stopped happening
 * still reads as evidence. That is not hypothetical: a control transcribing a
 * suite's own terminal block goes stale the moment a case is added to that
 * suite, while every count beside it stays internally consistent.
 *
 * So every control is re-applied rather than believed. This module owns the
 * part both lanes share — which lane owns a control, and the plant/run/restore
 * routine itself — so the rule is stated once and the two entry points cannot
 * drift into covering overlapping or partial sets.
 *
 * The split between them is a toolchain boundary, not a sample:
 *
 *   - `instrument` — the control's bound record runs under `node`, and its
 *     command is not the instrument suite itself. The instrument suite drives
 *     these, so the fast roadmap lane re-applies them on every change.
 *   - `control-lane` — everything else: the records that run under `cargo`,
 *     and the one whose command IS the instrument suite. Driving the latter
 *     from the suite it runs would re-enter that suite; driving it from a
 *     different entry point terminates by construction, because the suite it
 *     spawns drives no control whose command is itself.
 *
 * What no control may be is TRANSCRIBED. A control that mutates no artifact
 * still has two things only a run can re-derive: whether its record's command
 * still runs clean on this tree, and the refusal its own delta produces — an
 * empty selection reports that it selected nothing, which is a property of
 * the tree's current test inventory and not only of the runner. So a
 * command-shaped control is re-applied by the lane its record's runner
 * already decides, beside the controls that mutate files, and its record's
 * command runs clean first under the same evidence rule as any other.
 * The cost that once made that class transcribed was the cold build it
 * seemed to require; the control lane builds into a target directory of its
 * own under the checkout's CI-cached target root, from a mirror at a stable
 * path, so a command-shaped control re-runs what its lane builds anyway
 * rather than rebuilding the heaviest packages from nothing — and the exes it
 * re-runs are baked with source roots that stay valid, instead of whichever
 * tree last wrote into a shared target directory.
 *
 * What a re-application is judged AGAINST is the tree it runs on, never the
 * numbers a record wrote down once. A record's transcript is its own claim,
 * and the validator holds the record's five counts to that transcript; but
 * the clean run here is fresh evidence, and fresh evidence is judged on what
 * it establishes about THIS tree: it ran to the runner's own terminal
 * summary, it executed work, it reports zero failures, every skip it reports
 * is one the tree itself declares, and the work it ran is the work the tree
 * currently selects for the record's command — re-derived from the runner's
 * own inventory of that selection where the runner has one, not from a count
 * transcribed on some other day. Comparing the live counters against the
 * transcribed ones would turn every case added to a selected suite, and every
 * `cfg`-gated case that compiles on one host and not another, into a red lane
 * whose only repair is rewriting the number — a maintenance ritual that
 * proves nothing about the mutation and blocks the tree until someone
 * performs it. The refusal is held to the same rule: the mutation's own
 * signature — how many cases it broke, whether it emptied the selection,
 * which errors it printed — must be the one the control transcribes, while
 * the suite-size counters beside it (how many cases still passed, how many
 * were ignored) are properties of the tree, not of the mutation.
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
 * The control lane's cargo target directory persists across runs, and cargo
 * decides freshness by comparing each source file's mtime against the
 * artifact it last built from it. A mutated run builds an artifact from the
 * planted source, the plant is restored, and the next run recreates the
 * mirror from a copy — a copy that, on Windows, keeps the checkout's older
 * timestamps (`fs.cpSync` goes through `CopyFile`, which preserves them). To
 * cargo the artifact built from the MUTATED source is then newer than every
 * source it depends on, so it is reused, and the clean run fails on the
 * previous run's mutation: a refusal no edit in this run produced, on a tree
 * that is not red. A mirror younger than anything the target directory holds
 * is what makes those fingerprints decide from the bytes actually on disk, on
 * every platform. Linked dependency trees are left alone: nothing is built
 * from them and they are not this mirror's copy.
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

/**
 * The control lane's own entry point and the command line CI issues for it.
 *
 * Held here rather than in either suite so the instrument suite resolves the
 * lane it delegates to against the same two values the lane itself is, and a
 * renamed entry fails on both sides at once instead of leaving a workflow job
 * pointing at a file nobody runs.
 */
export const CONTROL_LANE_ENTRY = "roadmap/0.1.0-tama/tools/closure-controls.test.mjs";
export const CONTROL_LANE_COMMAND = `node --test ${CONTROL_LANE_ENTRY}`;

/**
 * Each lane's own deadline, and the per-command deadline nested inside it.
 *
 * Declared here rather than written into the suites because the instrument
 * suite resolves them against each hosting job's `timeout-minutes`. A child
 * deadline at or above its parent killer is the failure mode where the job is
 * terminated by the runner with no diagnostic at all: the suite never reaches
 * its own timeout, so nothing says which control was still running. Every
 * mirror a lane builds and every command it spawns therefore has to fit
 * strictly inside the budget the workflow declares, and that nesting is
 * checked rather than commented.
 */
export const CONTROL_LANE_DEADLINE_MS = 40 * 60_000;
export const CONTROL_LANE_COMMAND_DEADLINE_MS = 20 * 60_000;
export const INSTRUMENT_LANE_DEADLINE_MS = 10 * 60_000;
export const INSTRUMENT_LANE_COMMAND_DEADLINE_MS = 2 * 60_000;

/**
 * The preload that turns a node:test suite file into its own inventory,
 * spelled relative to the repository root every lane runs its commands from.
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

/** The two lanes that between them re-apply every control in the register. */
export const INSTRUMENT_LANE = "instrument";
export const CONTROL_LANE = "control-lane";

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
 * Which lane owns a control.
 *
 * The record's runner and entry decide, never the control's KIND: a
 * command-shaped control is re-applied by the same lane that would own it
 * were its mutation a file edit, so no control is exempt from re-application
 * for mutating no artifact.
 *
 * `instrumentEntry` is the repo-relative path of the instrument suite, passed
 * in rather than hard-coded so the rule is resolved against the file actually
 * running instead of a spelling that could go stale.
 */
export function laneFor(model, control, instrumentEntry) {
  const { adapter, argv } = controlCommand(model, control);
  if (adapter.runner !== "node") return CONTROL_LANE;
  return argv.includes(instrumentEntry) ? CONTROL_LANE : INSTRUMENT_LANE;
}

/** The controls a lane owns, in register order. */
export const controlsFor = (model, lane, instrumentEntry) =>
  model.register.control.filter((control) => laneFor(model, control, instrumentEntry) === lane);

/**
 * The command a runner exposes to LIST the selection a run command would
 * execute, or `null` for a runner that has none.
 *
 * This is the tree's own inventory of the record's selection, produced by the
 * same runner and the same selection arguments as the clean run, so a clean
 * run can be held to what the tree currently selects instead of to a number
 * transcribed once. `cargo nextest list` re-derives the selection from the
 * built binaries and reports, per case, whether it matches the run's filter —
 * which is exactly the split the run summary reports as executed and skipped.
 * libtest's `--list` prints every case the same filter admits, ignored ones
 * included; the ignored subset is listed on request with `--ignored`.
 * node:test has no listing mode, so its inventory is the suite's own module
 * code run under {@link NODE_TEST_INVENTORY_PRELOAD}, which counts every case
 * the suite declares and runs none of them. A tool printing one verdict line
 * lists nothing either; where its count is a selection the tree declares,
 * {@link TOOL_LINE_INVENTORIES} names the script that re-derives it.
 *
 * A runner with no inventory at all — a single-verdict tool absent from that
 * table — leaves the clean run judged on its own summary alone: zero
 * failures, nonzero work, and no skip the record does not declare.
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
    // The run command is `nextest run …`; the listing is `nextest list …` over
    // the same selection, and the format flag is a `list` option that has to
    // precede any `--` the tail may carry.
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
 *
 * nextest lists every case in the selected binaries and marks per case
 * whether the run's filter matches it; the executed count of the run is the
 * matching set, and everything else is what the summary counts as skipped.
 * libtest's listing states only the cases the filter admits, so its executed
 * and ignored split is read from a second, `--ignored`, listing when the
 * record declares that its selection carries ignored cases at all.
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
    // The preload's single exit line. Anything else the suite's module code
    // prints around it is not the inventory, and two such lines mean two
    // processes' counts were interleaved into one listing.
    const rows = [
      ...output
        .replaceAll("\r\n", "\n")
        .matchAll(/^node-test inventory: declared (\d+) skipped (\d+)$/gmu),
    ];
    if (rows.length !== 1) return null;
    return { selected: Number(rows[0][1]), skipped: Number(rows[0][2]) };
  }
  if (grammar === "tool-line") {
    // The inventory script's one line, naming the key its count is under so
    // it is compared only against the field the record counts.
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
 * selects by filter.
 *
 * An exact selection states the record's intended work in the command itself,
 * so the clean run is held to it: a named case that no longer exists on the
 * tree leaves the run green over fewer cases, which is a selection the record
 * did not intend rather than a suite that grew.
 */
export function exactSelection(argv) {
  const separator = argv.indexOf("--");
  if (separator === -1) return null;
  const tail = argv.slice(separator + 1);
  if (!tail.includes("--exact")) return null;
  return tail.filter((token) => !token.startsWith("-"));
}

/**
 * The part of a refusal that is the MUTATION's, with the suite-size counters
 * beside it dropped.
 *
 * A mutation's signature is how many cases it broke, whether it emptied the
 * selection, and which errors it made a tool print. How many cases still
 * passed around it, and how many were ignored, describe the tree the mutation
 * was planted in — they move whenever a case is added to the suite or a
 * `cfg`-gated case compiles on a different host, and a refusal comparison that
 * reads them turns every such change into a control whose only repair is
 * rewriting its transcript.
 */
export function refusalSignature(grammar, refusal) {
  if (!refusal) return null;
  if (grammar === "libtest") return { failing: refusal.failing, failed: refusal.failed };
  if (grammar === "nextest")
    return { failed: refusal.failed, selectedNothing: refusal.selectedNothing === true };
  if (grammar === "node-test" || grammar === "compile-contracts") return { failed: refusal.failed };
  if (grammar === "tool-line") return { errors: refusal.errors };
  return null;
}

/**
 * What a clean run has to establish about the tree it ran on.
 *
 * The record's transcribed counters are NOT among the things it is compared
 * against: those are the record's claim about a run somebody once did, and
 * the validator already holds the record's five numbers to that transcript.
 * A live run is judged on what it proves now — it reached the runner's own
 * summary with zero failures, it executed work, every skip it reports is one
 * the record declares its selection carries AND one the tree's own inventory
 * marks ignored, and the work it ran is the work the tree currently selects
 * for the command.
 */
function assertCleanRunEvidence({ control, proof, adapter, argv, observedNow, inventory }) {
  const command = argv.join(" ");
  const grammar = adapter.summary_grammar;
  assert.equal(
    observedNow.failed,
    0,
    `${control.id}: the clean run of ${command} reports ${observedNow.failed} failed cases, so a refusal after the mutation would prove nothing`,
  );
  assert.ok(
    observedNow.executed >= 1,
    `${control.id}: the clean run of ${command} executed no case, so its selection is empty on this tree and a refusal after the mutation could be that same empty selection`,
  );
  const declaredSkips = proof.expected_skips ?? 0;
  if (declaredSkips === 0)
    assert.equal(
      observedNow.skipped,
      0,
      `${control.id}: the clean run of ${command} skipped ${observedNow.skipped} cases, and record ${proof.id} declares that its selection carries none`,
    );
  const exact = grammar === "libtest" ? exactSelection(argv) : null;
  if (exact)
    assert.equal(
      observedNow.executed,
      exact.length,
      `${control.id}: ${command} names ${exact.length} cases with --exact, but the clean run executed ${observedNow.executed}, so a case the record intends to run no longer exists on this tree`,
    );
  if (inventory === null) {
    assert.equal(
      declaredSkips,
      0,
      `${control.id}: record ${proof.id} declares ${declaredSkips} skips, but a ${grammar} runner exposes no inventory this lane could re-derive them from, so a skip on this tree cannot be told from an unexpected one`,
    );
    return;
  }
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

/**
 * Re-apply one control against a mirror and require the transcribed refusal to
 * be the one its command still produces.
 *
 * A source control's mutation is a file edit in the mirror; a command
 * control's is an argument appended to the recorded command. Both halves are
 * checked for the same property — provably applicable, provably absent — over
 * whatever the mutation replaces: the subject's text, or the argument vector.
 *
 * `spawn(argv, adapter)` is supplied by the caller because the two lanes launch
 * different runners: the instrument lane runs `node`, the control lane also
 * runs `cargo` with its own environment. The adapter is handed back rather than
 * re-derived so a caller dispatches on the runner the register declares instead
 * of guessing it from the argument vector, whose first element is a subcommand
 * rather than a program. Everything that decides whether the re-application
 * PROVED anything lives here.
 *
 * Three things are established, and only the third is about the mutation: the
 * record's command still runs clean on this tree — to its runner's own
 * summary, with zero failures, nonzero work, no undeclared skip, and over the
 * selection the tree's own inventory reports — the mutation was provably
 * applicable and provably absent before it was written, and the refusal the
 * mutated command emits carries the mutation's own signature, the one the
 * control transcribes.
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

  // That clean run is the bound record's OWN command, so its terminal summary
  // is fresh evidence about this tree, and it is judged as such: it must be a
  // summary of the runner's declared shape, report zero failures and nonzero
  // work, skip nothing the record does not declare, and — where the runner can
  // list its own selection — cover exactly the cases the tree selects for the
  // command today. What it is NOT compared against is the record's transcribed
  // counters: those are the record's claim about a run somebody once did, held
  // to its own transcript by the validator. A case added to the selected suite,
  // or a `cfg`-gated case that compiles on this host and not on the one the
  // transcript came from, moves the live counters without saying anything
  // about the mutation, and a lane that refused on that would be red until
  // someone rewrote the number.
  const observedNow = parseTerminalSummary(adapter.summary_grammar, cleanOutput, proof.count_key);
  assert.ok(
    observedNow,
    `${control.id}: the clean run of ${argv.join(" ")} emitted no ${adapter.summary_grammar} summary:\n${cleanOutput}`,
  );
  // A listing is read from STDOUT alone: cargo prints its build progress on
  // stderr around the runner's output, and a JSON listing with `Compiling`
  // lines appended to it is not JSON. The whole output still goes into the
  // failure message, so a refused listing says why.
  let inventory = null;
  const listing = inventoryCommand(adapter, argv);
  if (listing) {
    const listed = spawn(listing, adapter);
    const listedOutput = `${listed.stdout ?? ""}${listed.stderr ?? ""}`;
    assert.equal(
      listed.status,
      0,
      `${control.id}: the runner refused to list the selection of ${argv.join(" ")}, so the clean run cannot be held to the tree's own inventory\n${listedOutput}`,
    );
    inventory = parseInventory(adapter.summary_grammar, listed.stdout ?? "");
    assert.ok(
      inventory,
      `${control.id}: ${listing.join(" ")} emitted no ${adapter.summary_grammar} listing:\n${listedOutput}`,
    );
    // libtest lists ignored cases beside the runnable ones without marking
    // them, so a record that declares ignored cases in its selection gets
    // that subset listed on its own; a record declaring none needs no second
    // listing, because the summary's own skip count is already required to
    // be zero.
    if (adapter.summary_grammar === "libtest" && (proof.expected_skips ?? 0) > 0) {
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
  }
  assertCleanRunEvidence({ control, proof, adapter, argv, observedNow, inventory });

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
  assert.notEqual(
    mutated.status,
    0,
    `${control.id}: ${argv.join(" ")} accepted the mutation\n${output}`,
  );
  const live = parseRefusal(adapter.summary_grammar, output, proof.count_key);
  assert.ok(
    live,
    `${control.id}: the mutated run emitted no ${adapter.summary_grammar} refusal:\n${output}`,
  );
  // The mutation's own signature, not the whole refusal: how many cases it
  // broke, whether it emptied the selection, which errors it printed. The
  // suite-size counters beside those — passed, ignored, skipped — describe
  // the tree the mutation was planted in, and the clean run above has already
  // held that tree to its own inventory.
  assert.deepEqual(
    refusalSignature(adapter.summary_grammar, live),
    refusalSignature(
      adapter.summary_grammar,
      parseRefusal(adapter.summary_grammar, control.observed, proof.count_key),
    ),
    `${control.id}: the refusal this control transcribes is no longer the one its command produces:\n${output}`,
  );
  return { proof, adapter, argv, output };
}
