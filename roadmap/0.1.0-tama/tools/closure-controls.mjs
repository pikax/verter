/**
 * On-demand negative-control replay. Routine CI runs focused fixture tests
 * and the normal test lanes; it does not replay historical evidence records.
 * A replay requires a nonempty, complete clean run, an applicable mutation,
 * and the expected refusal. Passing totals are observations of the current
 * inventory, never values that must match an old transcript.
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

/** Entry point for the manually dispatched diagnostic workflow. */
export const CONTROL_LANE_ENTRY = "roadmap/0.1.0-tama/tools/closure-controls.test.mjs";
export const CONTROL_LANE_DEADLINE_MS = 40 * 60_000;
export const CONTROL_LANE_COMMAND_DEADLINE_MS = 20 * 60_000;

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
