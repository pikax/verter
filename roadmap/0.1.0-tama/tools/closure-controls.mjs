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
 * still has two things only a run can re-derive: the counters its record
 * transcribes, and the refusal its own delta produces — an empty selection
 * reports how many tests it failed to select, which is a property of the
 * tree's current test inventory and not only of the runner. So a
 * command-shaped control is re-applied by the lane its record's runner
 * already decides, beside the controls that mutate files, and its record's
 * command runs clean first under the same counter comparison as any other.
 * The cost that once made that class transcribed was the cold build it
 * seemed to require; the control lane builds into a target directory of its
 * own under the checkout's CI-cached target root, from a mirror at a stable
 * path, so a command-shaped control re-runs what its lane builds anyway
 * rather than rebuilding the heaviest packages from nothing — and the exes it
 * re-runs are baked with source roots that stay valid, instead of whichever
 * tree last wrote into a shared target directory.
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
 * record's transcribed counters are the ones its command still produces, the
 * mutation was provably applicable and provably absent before it was written,
 * and the refusal the control transcribes is the one the mutated command still
 * emits.
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
  // is the record's own counters, produced now rather than transcribed once.
  // Comparing them here is what makes a record's numbers re-derivable for every
  // record whose control a lane drives: a case added to, or deleted from, the
  // selection the record names moves this summary and fails, instead of leaving
  // a self-consistent transcript describing a suite that no longer exists. The
  // record's five declared counts are separately checked against its own
  // transcribed text by the validator, so the pair is closed: the transcript
  // states what the record claims, and the transcript is what the command still
  // emits.
  const transcribed = parseTerminalSummary(
    adapter.summary_grammar,
    proof.terminal_summary,
    proof.count_key,
  );
  assert.ok(
    transcribed,
    `${control.id}: record ${proof.id} transcribes no ${adapter.summary_grammar} summary to compare a live run against`,
  );
  const observedNow = parseTerminalSummary(adapter.summary_grammar, cleanOutput, proof.count_key);
  assert.ok(
    observedNow,
    `${control.id}: the clean run of ${argv.join(" ")} emitted no ${adapter.summary_grammar} summary:\n${cleanOutput}`,
  );
  assert.deepEqual(
    observedNow,
    transcribed,
    `${control.id}: record ${proof.id} transcribes counters its own command no longer produces (host=${process.platform}):\n${cleanOutput}`,
  );

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
  assert.deepEqual(
    live,
    parseRefusal(adapter.summary_grammar, control.observed, proof.count_key),
    `${control.id}: the refusal this control transcribes is no longer the one its command produces:\n${output}`,
  );
  return { proof, adapter, argv, output };
}
