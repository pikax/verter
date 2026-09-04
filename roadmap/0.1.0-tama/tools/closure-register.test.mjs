// Negative controls for the closure-register instrument.
//
// Every case here is a mutation applied to a hermetic fixture package root:
// the baseline is built, asserted clean, then exactly one property is broken
// and the validator must refuse it. A mutation that the validator accepts is
// the failure this suite exists to catch — the instrument would then be
// reporting a pass for a register that does not hold.
//
// The fixture is synthetic on purpose. It never reads the live register, so a
// control cannot be satisfied by whatever the live register happens to say
// today, and the live register cannot be repaired by weakening a control.

import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import test, { after, afterEach } from "node:test";
import { fileURLToPath } from "node:url";

import {
  ALLOWED_RESIDUES,
  CI_TRIGGER_FILTER,
  CI_WORKFLOW,
  INSTRUMENT_COMMANDS,
  LIVE_UNIVERSE,
  MANDATORY_CONTROL_CLASSES,
  statementDigest,
  MUST_CLOSE_FINDINGS,
  OPEN,
  PROVEN,
  PROVEN_BOUNDED,
  READY_FOR_REVIEW,
  REFUSED,
  RESIDUE_FINDINGS,
  SHIPPED_OBLIGATION_AUTHORITY_ONLY,
  SHIPPED_OBLIGATION_CARRIED,
  SHIPPED_OBLIGATION_MET,
  SUMMARY_GRAMMARS,
  VIEW_RELATIVE,
  analyze,
  anchorDigest,
  certification,
  ownerCapability,
  parseRefusal,
  parseTerminalSummary,
  publication,
  laneCommandLine,
  triggerCovers,
  triggerPaths,
  workflowJobs,
  renderView,
  selectedPackages,
  viewIsFresh,
} from "./closure-register.mjs";
import {
  CONTROL_LANE,
  CONTROL_LANE_COMMAND,
  CONTROL_LANE_DEADLINE_MS,
  CONTROL_LANE_ENTRY,
  INSTRUMENT_LANE,
  INSTRUMENT_LANE_COMMAND_DEADLINE_MS,
  INSTRUMENT_LANE_DEADLINE_MS,
  controlCommand,
  controlsFor,
  laneFor,
  linkInstalledModuleTrees,
  reapply,
} from "./closure-controls.mjs";
import { PACKAGE_ROOT } from "./lib.mjs";

// --- failure-class registration -------------------------------------------
// Each control declares the class it discriminates. The final case asserts the
// registered set equals the validator's declared closed set exactly, so a
// dropped control fails rather than silently shrinking the suite.
const REGISTERED = new Set();

// The cases that assert over this suite's own declarations, or over the live
// tree without mutating it. Pinned so that a case which stops planting a
// mutation cannot migrate into the non-planting set without a reviewer seeing
// the name arrive here.
const DECLARED_SELF_ASSERTIONS = Object.freeze([
  "the fixture baseline validates and derives a reviewable state",
  "every re-derivable evidence record is re-executed against its transcription",
  "every control this instrument can drive is re-applied to a mirror of the tree and refused",
  "every control is owned by a re-applying lane, and that lane runs in CI",
  "the re-application routine plants and refuses what the register says",
  "the registered controls are exactly the declared mandatory classes",
  "the closed universes are pinned, not derived from the register",
  "the live register validates, is reviewable, and never derives an admissible-by-fiat state",
  "the transcribed counters for observable records still match reality",
]);

// node:test registers every top-level case synchronously at import, so this
// count is complete before the first body runs. The live-register case compares
// it against the selected-work the register claims for this suite: adding or
// removing a case without re-running and re-transcribing the record fails.
let DECLARED_CASES = 0;

// A control's admissibility rests on a property of its BODY, not of this file:
// the suite may import the module its claim is about because an adversarial
// case passes only when that module REFUSES a planted mutation, so a permissive
// module fails it. Stating that of the file left the property unenforced — a
// case added here later could be an ordinary positive assertion over the module
// it is exempted from judging, and nothing would notice.
//
// So both halves of the property are derived per case, and both are derived
// from what happened rather than from what was supplied.
//
//   - The mutation is counted when the fixture it produced actually DIFFERS
//     from the baseline. Counting the callback instead made an empty callback
//     satisfy the gate: `run(() => {})` supplies a mutator, plants nothing, and
//     was indistinguishable from a real plant.
//   - The refusal is counted at the places that DERIVE one — an error carrying
//     a named fragment, a derived limit on a named record, a certification the
//     tool declines, a view that no longer reads as fresh. A case that plants a
//     mutation and then asserts the validator was fine with it is not a
//     control, and stating "controls pass only when the validator refuses"
//     while nothing checked it was the same unenforced claim one level down.
//
// The cases that legitimately assert over this suite's own declarations, or
// over the live tree without mutating it, are registered through
// `selfAssertion` instead: separately counted and separately named, visible
// rather than absorbed into the exemption.
let MUTATIONS_PLANTED = 0;
let REFUSALS_OBSERVED = 0;
const SELF_ASSERTIONS = new Set();

const control = (name, body) => {
  DECLARED_CASES += 1;
  test(name, async (t) => {
    const plantedBefore = MUTATIONS_PLANTED;
    const refusedBefore = REFUSALS_OBSERVED;
    await body(t);
    assert.ok(
      MUTATIONS_PLANTED > plantedBefore,
      `control "${name}" produced no fixture that differs from the baseline, so it planted nothing; register it through selfAssertion if it asserts over this suite's declarations or over the unmutated tree`,
    );
    assert.ok(
      REFUSALS_OBSERVED > refusedBefore,
      `control "${name}" planted a mutation and observed no refusal, so it does not carry the property that admits this suite's import of its own subject`,
    );
  });
};

const selfAssertion = (name, body, options = {}) => {
  DECLARED_CASES += 1;
  SELF_ASSERTIONS.add(name);
  test(name, options, body);
};

/**
 * The `timeout-minutes` a workflow job declares, in milliseconds.
 *
 * A job budget is a KILLER, not a limit: a deadline inside the suite that is
 * at or above it never fires, so the run ends as a runner termination with no
 * diagnostic naming what was still going. The nesting is therefore resolved
 * out of the workflow rather than assumed.
 */
const jobBudgetMs = (body) => {
  const row = body.match(/^\s*timeout-minutes:\s*(\d+)\s*$/mu);
  return row ? Number(row[1]) * 60_000 : null;
};

// `DECLARED_CASES` counts registrations through the wrapper above, so on its own
// it is a convention: a case added with a bare `test(...)` would make the runner
// report one more than the register transcribes while the wrapper's count still
// agreed with the record. The root `afterEach` hook runs for EVERY top-level
// case however it was registered, so the two counts are reconciled against the
// runner itself rather than against the wrapper.
let EXECUTED_CASES = 0;
afterEach(() => {
  EXECUTED_CASES += 1;
});
after(() => {
  assert.equal(
    EXECUTED_CASES,
    DECLARED_CASES,
    `the runner executed ${EXECUTED_CASES} cases but only ${DECLARED_CASES} were registered through the control wrapper; a case registered outside it is invisible to the transcription check`,
  );
});

const covers = (failureClass) => {
  assert.ok(
    MANDATORY_CONTROL_CLASSES.includes(failureClass),
    `${failureClass} is not a declared failure class`,
  );
  REGISTERED.add(failureClass);
};

// --- minimal TOML emitter --------------------------------------------------
// The roadmap tools read a TOML subset: scalars, single-line arrays, `[table]`
// and `[[array-table]]` sections. The fixture only needs that subset.
function tomlValue(value) {
  if (typeof value === "string") return JSON.stringify(value);
  if (typeof value === "number") return String(value);
  if (typeof value === "boolean") return String(value);
  if (Array.isArray(value)) return `[${value.map(tomlValue).join(", ")}]`;
  throw new Error(`fixture: unsupported TOML value ${JSON.stringify(value)}`);
}

function tomlBody(row) {
  return Object.entries(row).map(([key, value]) => `${key} = ${tomlValue(value)}`);
}

function toToml(model) {
  const lines = [];
  for (const [key, value] of Object.entries(model)) {
    if (
      Array.isArray(value) &&
      value.every((item) => item && typeof item === "object" && !Array.isArray(item))
    )
      continue;
    if (value && typeof value === "object" && !Array.isArray(value)) continue;
    lines.push(`${key} = ${tomlValue(value)}`);
  }
  for (const [key, value] of Object.entries(model)) {
    if (!value || typeof value !== "object" || Array.isArray(value)) continue;
    lines.push("", `[${key}]`, ...tomlBody(value));
  }
  for (const [key, value] of Object.entries(model)) {
    if (!Array.isArray(value)) continue;
    if (!value.every((item) => item && typeof item === "object" && !Array.isArray(item))) continue;
    for (const row of value) lines.push("", `[[${key}]]`, ...tomlBody(row));
  }
  return `${lines.join("\n")}\n`;
}

// --- fixture ---------------------------------------------------------------

/**
 * A refusal transcript in every runner grammar at once.
 *
 * A fixture control row is read under whichever adapter the case under test
 * points its record at — `tool-line` in the baseline, `libtest` once a case
 * turns that adapter into a cargo runner, `node-test` once one gives a record a
 * skip count. The recorded outcome has to be the RUNNER's own refusal under
 * each of them, so this synthetic fixture carries every shape rather than a
 * different control row per case.
 */
const fixtureRefusal = (what) =>
  [
    `ERROR: ${what}`,
    "test result: FAILED. 3 passed; 1 failed; 0 ignored; 0 measured; 0 filtered out",
    "tests 5 | pass 1 | fail 1 | cancelled 0 | skipped 3 | todo 0",
    "Summary [ 1.000s] 4 tests run: 3 passed, 1 failed, 0 skipped",
    "compile contracts: owner=fixture, fixtures=2",
  ].join("\n");

const STATED_ANCHOR = "the fixture states this obligation verbatim";
const OPEN_ANCHOR = "the fixture leaves this question open";

const FIXTURE_CONTRACT = `# Fixture contract

## 1. Stated

The stated obligation: ${STATED_ANCHOR}, with a body so the section is not empty.

## 2. Open

The obligation this fixture transfers to a remainder, because ${OPEN_ANCHOR}.

## 3. Gamma

A third obligation with its own section body.
`;

/**
 * The file the fixture's source-shaped mutations were applied to.
 *
 * The repeated pair at the end is deliberate: it is what a control needs in
 * order to plant a mutation whose replaced text is NOT unique, which is the
 * half of `unique-new-occurrence` that used to be an author's word.
 */
const FIXTURE_SUBJECT = "tools/fixture-subject.mjs";
const FIXTURE_SUBJECT_TEXT = `export const alpha = 1;
export const beta = 2;
export const gamma = 3;
export const repeated = 0;
export const tail = 9;
export const repeated = 0;
export const tail = 9;
`;

/**
 * Every charter in the program declares the same four criterion slots with the
 * same four roles, which is exactly why a bare ordinal discriminates nothing.
 * The fixture reproduces that shape, so a control that rebinds a citation to a
 * different ordinal exercises the real ambiguity rather than an invented one.
 */
const FIXTURE_ROLES = [
  "sole-owner outcome",
  "positive contract",
  "incremental equivalence",
  "bounded work",
];

const FIXTURE_OWNER = "fixture:ratified owner";
const FIXTURE_DISPLACED_OWNER = "fixture:displaced owner";

/**
 * A charter states its owner twice, and the two statements are written by
 * different hands. `owner=` is a generated header field; the outcome sentence
 * is prose a reader meets first, naming the owner being displaced and the one
 * that ends up sole. The fixture reproduces both, because a header regenerated
 * over untouched prose is the defect this instrument closes and a fixture
 * carrying only the header could not express it.
 *
 * `role` is the node's semantic role. A historical identity wrapper delivers no
 * outcome and narrates none, so the fixture can express that shape too and the
 * exemption is exercised rather than assumed.
 */
function charter(nodeId, count = 4, owner = FIXTURE_OWNER, narrative = {}) {
  const criteria = Array.from(
    { length: count },
    (_, index) => `- **${nodeId}-AC${index + 1} — ${FIXTURE_ROLES[index]}:** fixture criterion.`,
  );
  const {
    role = "delivery",
    displaced = FIXTURE_DISPLACED_OWNER,
    final = ownerCapability(FIXTURE_OWNER),
    // A second pair inside the owning section, or the section written twice:
    // both are shapes a first-match reader of the whole document resolves to
    // one answer while a reader of the charter meets two.
    extraOutcome = "",
    surfaces = ["crates/fixture-owned/src"],
  } = narrative;
  const pair = `The current owner is **${displaced}**. The final and sole owner is **${final}**.`;
  const outcome =
    role === "delivery"
      ? `\n## Independently acceptable outcome\n\nFixture outcome. ${pair} This charter accepts one boundary.\n`
      : "";
  const declared = surfaces.map((surface) => `\`${surface}\``).join(", ");
  return (
    // A null owner writes the charter WITHOUT the generated header, which is
    // the shape a charter has before regeneration: the prose can still narrate
    // an owner while the field a reader's tooling resolves is simply absent.
    `# ${nodeId}\n\n${owner === null ? "" : `owner=${owner}\n`}${outcome}${extraOutcome}\n` +
    `## Concrete surfaces and APIs\n\n- Production surfaces: ${declared}.\n- Mutation boundary: fixture boundary.\n\n` +
    `## Acceptance\n\n${criteria.join("\n")}\n`
  );
}

/**
 * The lanes that would notice a change to the artifacts the register cites.
 *
 * The validator resolves both: this instrument's own lane must run its own
 * commands under the `tama` filter, and an external record's declared lane must
 * exist, issue its declared command, and be gated on the filter its cited
 * artifacts are then measured against. The fixture reproduces both shapes so a
 * control can drop a pattern, unhook a gate, or point a record at a lane that
 * does not run its runner.
 */
const FIXTURE_REFRESH_JOB = "rust-lane";
const FIXTURE_REFRESH_COMMAND = "cargo nextest run --workspace --fixture";
const FIXTURE_WORKFLOW = `name: Fixture CI

on:
  pull_request:

jobs:
  detect-changes:
    runs-on: ubuntu-latest
    steps:
      - uses: dorny/paths-filter@v4
        with:
          filters: |
            tama:
              - 'contracts/**'
              - 'decisions/**'
              - 'tools/**'
              - 'crates/**'
            rust:
              - 'crates/**'
              # A lane's own selection script is an input to that lane, exactly
              # as the live rust filter covers the script its core filter is
              # computed from.
              - 'tools/**'

  tama-roadmap:
    needs: detect-changes
    if: needs.detect-changes.outputs.tama == 'true'
    runs-on: ubuntu-latest
    steps:
      - run: |
          ${INSTRUMENT_COMMANDS.join("\n          ")}

  ${FIXTURE_REFRESH_JOB}:
    needs: detect-changes
    if: needs.detect-changes.outputs.rust == 'true'
    runs-on: ubuntu-latest
    steps:
      - run: ${FIXTURE_REFRESH_COMMAND}
`;

/** The register the whole suite mutates. Valid as written. */
function baselineRegister() {
  const findings = [
    ...MUST_CLOSE_FINDINGS.map((id) => ({
      id,
      statement: `${id} fixture statement.`,
      claim: "CLM-ALPHA",
      atom: "A-plain",
    })),
    ...Object.entries(RESIDUE_FINDINGS).map(([id, residue]) => ({
      id,
      statement: `${id} fixture statement.`,
      residue,
    })),
  ];
  return {
    schema: 1,
    instrument: "control-fixture",
    ratification: {
      owner: FIXTURE_OWNER,
      displaced_owner: "fixture:displaced owner",
      contract: "contracts/fixture.md",
      instrument_authority: "charters/TCM0R.md",
      decision: "decisions/fixture.md",
    },
    adapter: [
      {
        id: "node-tool",
        runner: "node",
        argv_prefix: [],
        summary_shape: "one terminal line",
        summary_grammar: "tool-line",
        reexecution: "instrument",
      },
    ],
    claim: [
      { id: "CLM-ALPHA", statement: "Alpha claim.", subject: ["contracts/fixture.md"] },
      { id: "CLM-BETA", statement: "Beta claim.", subject: ["contracts/fixture.md"] },
      { id: "CLM-GAMMA", statement: "Gamma claim.", subject: ["decisions/fixture.md"] },
    ],
    atom: [
      {
        id: "A-stated",
        claim: "CLM-ALPHA",
        statement: "A stated contract obligation.",
        contract_section: "1. Stated",
        contract_anchor: STATED_ANCHOR,
        evidence_anchor: "contracts/fixture.md",
        shipped_obligation: "met",
        received_by: "TCM1-AC1",
        received_by_role: "sole-owner outcome",
      },
      {
        id: "A-plain",
        claim: "CLM-ALPHA",
        statement: "An obligation this block proves outright.",
        evidence_anchor: "contracts/fixture.md",
        shipped_obligation: "met",
      },
      {
        id: "A-beta",
        claim: "CLM-BETA",
        statement: "A covered beta obligation.",
        evidence_anchor: "contracts/fixture.md",
        shipped_obligation: "met",
      },
      {
        id: "A-hang",
        claim: "CLM-BETA",
        statement: "Transferred to the hang remainder.",
        contract_section: "2. Open",
        contract_anchor: OPEN_ANCHOR,
        evidence_anchor: "contracts/fixture.md",
        shipped_obligation: "carried",
      },
      {
        id: "A-topology",
        claim: "CLM-BETA",
        statement: "Transferred to the topology remainder.",
        evidence_anchor: "contracts/fixture.md",
        shipped_obligation: "carried",
      },
      {
        id: "A-baseline",
        claim: "CLM-BETA",
        statement: "Transferred to the baseline remainder.",
        evidence_anchor: "contracts/fixture.md",
        shipped_obligation: "carried",
      },
      {
        // Anchored at the artifact only `P-gamma` runs. That is what lets a
        // control plant a real, green, unrelated record against this atom and
        // see it refused.
        id: "A-gamma",
        claim: "CLM-GAMMA",
        statement: "A covered gamma obligation.",
        evidence_anchor: "tools/fixture-gamma.mjs",
        shipped_obligation: "met",
      },
    ],
    proof: [
      {
        id: "P-alpha",
        adapter: "node-tool",
        argv_tail: ["fixture-alpha.mjs"],
        fixtures: ["contracts/fixture.md"],
        covers: ["A-stated", "A-plain"],
        selected: 4,
        executed: 4,
        passed: 4,
        failed: 0,
        skipped: 0,
        terminal_summary: "fixture-alpha: PASS cases=4",
        count_key: "cases",
        control: "CTL-alpha",
      },
      {
        id: "P-beta",
        adapter: "node-tool",
        argv_tail: ["fixture-beta.mjs"],
        fixtures: ["contracts/fixture.md"],
        covers: ["A-beta"],
        selected: 2,
        executed: 2,
        passed: 2,
        failed: 0,
        skipped: 0,
        terminal_summary: "fixture-beta: PASS cases=2",
        count_key: "cases",
        control: "CTL-beta",
      },
      {
        id: "P-gamma",
        adapter: "node-tool",
        // Path-shaped, so this record has a derived verdict producer: the
        // acyclicity control makes it a claim subject and the coverage must
        // then be refused.
        argv_tail: ["tools/fixture-gamma.mjs"],
        fixtures: ["contracts/fixture.md"],
        covers: ["A-gamma"],
        selected: 1,
        executed: 1,
        passed: 1,
        failed: 0,
        skipped: 0,
        terminal_summary: "fixture-gamma: PASS cases=1",
        count_key: "cases",
        control: "CTL-gamma",
      },
    ],
    control: [
      {
        id: "CTL-alpha",
        kind: "source",
        mutation: "Break alpha.",
        subject: FIXTURE_SUBJECT,
        reverted: "export const alpha = 1;\nexport const beta = 2;",
        applied: "export const alpha = 0;\nexport const beta = 2;",
        uniqueness: "unique-new-occurrence",
        observed: fixtureRefusal("fixture-alpha refused the mutated subject"),
      },
      {
        id: "CTL-beta",
        kind: "source",
        mutation: "Break beta.",
        subject: FIXTURE_SUBJECT,
        reverted: "export const beta = 2;\nexport const gamma = 3;",
        applied: "export const beta = 0;\nexport const gamma = 3;",
        uniqueness: "unique-new-occurrence",
        observed: fixtureRefusal("fixture-beta refused the mutated subject"),
      },
      {
        id: "CTL-gamma",
        kind: "command",
        mutation: "Re-run gamma with a selector that matches nothing.",
        argv_delta: ["--fixture-filter", "matches-nothing"],
        uniqueness: "unique-new-occurrence",
        observed: fixtureRefusal("fixture-gamma selected no work under the added selector"),
      },
    ],
    finding: findings,
    residue: ALLOWED_RESIDUES.map((id) => ({ id, statement: `${id} fixture remainder.` })),
    receiving: [
      {
        residue: ALLOWED_RESIDUES[0],
        order: 1,
        owner_node: "TCM1",
        criterion: "TCM1-AC1",
        criterion_role: "sole-owner outcome",
        gate: "TCM1 acceptance: fixture gate.",
      },
      {
        residue: ALLOWED_RESIDUES[0],
        order: 2,
        owner_node: "TCM1",
        criterion: "TCM1-AC2",
        criterion_role: "positive contract",
        gate: "TCM1 acceptance: fixture gate.",
      },
      {
        residue: ALLOWED_RESIDUES[1],
        order: 1,
        owner_node: "TCM1",
        criterion: "TCM1-AC3",
        criterion_role: "incremental equivalence",
        gate: "TCM1 acceptance: fixture gate.",
      },
      {
        residue: ALLOWED_RESIDUES[2],
        order: 1,
        owner_node: "TCM1",
        criterion: "TCM1-AC4",
        criterion_role: "bounded work",
        gate: "TCM1 acceptance: fixture gate.",
      },
    ],
    transfer: [
      { atom: "A-hang", residue: ALLOWED_RESIDUES[0], approved_by: "charters/TCM0R.md" },
      { atom: "A-topology", residue: ALLOWED_RESIDUES[1], approved_by: "charters/TCM0R.md" },
      { atom: "A-baseline", residue: ALLOWED_RESIDUES[2], approved_by: "charters/TCM0R.md" },
    ],
    row: [
      {
        kind: "deletion",
        subject: "Fixture displaced route",
        disposition: "Structurally rejected.",
        receiving_criterion: "TCM0R-AC1",
        receiving_criterion_role: "sole-owner outcome",
      },
      {
        kind: "survivor",
        subject: "Fixture retained artifact",
        disposition: "Retained as an obligation.",
        receiving_criterion: "TCM0R-AC2",
        receiving_criterion_role: "positive contract",
      },
    ],
  };
}

/**
 * The universe the fixture register is measured against, snapshotted once from
 * the clean baseline. Holding it fixed while the register is mutated is the
 * whole point: a claim, an atom, or a displaced-route row that disappears from
 * the register must fail against a universe it does not get to redefine.
 */
const FIXTURE_UNIVERSE = (() => {
  const baseline = baselineRegister();
  const claims = {};
  for (const claim of baseline.claim)
    claims[claim.id] = baseline.atom
      .filter((atom) => atom.claim === claim.id)
      .map((atom) => atom.id);
  const rows = {};
  for (const kind of ["deletion", "survivor"])
    rows[kind] = baseline.row.filter((row) => row.kind === kind).map((row) => row.subject);
  // The propositions, pinned the same way and for the same reason: the id sets
  // above do not move when a statement is rewritten under them. Row
  // dispositions and remainder statements are propositions too — they say how a
  // route was rejected and which question is carried — so they are pinned
  // beside the three that happen to carry an id.
  const statements = {};
  for (const claim of baseline.claim)
    statements[`claim:${claim.id}`] = statementDigest(claim.statement);
  for (const atom of baseline.atom) statements[`atom:${atom.id}`] = statementDigest(atom.statement);
  for (const finding of baseline.finding)
    statements[`finding:${finding.id}`] = statementDigest(finding.statement);
  for (const row of baseline.row)
    statements[`row:${row.subject}`] = statementDigest(row.disposition);
  for (const residue of baseline.residue)
    statements[`residue:${residue.id}`] = statementDigest(residue.statement);
  // A control's mutation and observed outcome, a receiving row's gate, and a
  // record's skip basis are propositions with no id of their own and were the
  // remaining half the pin skipped: each says what was demonstrated, what an
  // owner must clear, or why a skip is expected, and each was rewritable while
  // no id, count or derived status moved.
  for (const control of baseline.control) {
    statements[`control:${control.id}.mutation`] = statementDigest(control.mutation);
    statements[`control:${control.id}.observed`] = statementDigest(control.observed);
  }
  for (const row of baseline.receiving)
    statements[`receiving:${row.residue}#${row.order}.gate`] = statementDigest(row.gate);
  for (const proof of baseline.proof)
    if (proof.skip_basis)
      statements[`proof:${proof.id}.skip_basis`] = statementDigest(proof.skip_basis);
  // Where an atom points, pinned separately from what it says: the statement
  // digest cannot see a repoint, because the words do not move.
  const anchors = {};
  for (const atom of baseline.atom) anchors[`atom:${atom.id}`] = anchorDigest(atom);
  return Object.freeze({ claims, rows, statements, anchors });
})();

/**
 * The remainder TOPOLOGY the fixture register is measured against, snapshotted
 * the same way and for the same reason.
 *
 * The admissible-residue set alone does not pin the routing: an atom re-routed
 * to a different allowed remainder, or a required owner dropped from a
 * remainder's ordered sequence, passes every set-membership, descendant,
 * train, and role check. Holding the baseline's exact atom-to-residue and
 * residue-to-ordered-owner mappings fixed is what makes those substitutions
 * fail.
 */
const FIXTURE_TOPOLOGY = (() => {
  const baseline = baselineRegister();
  const transfers = {};
  for (const row of baseline.transfer) transfers[row.atom] = row.residue;
  const receiving = {};
  for (const row of [...baseline.receiving].sort((a, b) => a.order - b.order)) {
    if (!receiving[row.residue]) receiving[row.residue] = [];
    receiving[row.residue].push(row.owner_node);
  }
  return Object.freeze({ transfers, receiving });
})();

// Fixture roots outlive `run` so a control can inspect the tree afterwards —
// view freshness is a property of files on disk, not of the returned model.
const FIXTURE_ROOTS = [];
process.on("exit", () => {
  for (const root of FIXTURE_ROOTS) fs.rmSync(root, { recursive: true, force: true });
});

/** Materialize a fixture package root and run the validator against it. */
function run(mutate) {
  if (!mutate) mutate = () => {};
  const root = fs.mkdtempSync(path.join(os.tmpdir(), "closure-register-"));
  try {
    const register = baselineRegister();
    // The plant is what the mutator PRODUCED, not that it was supplied: either
    // the register it returns differs from the baseline, or it supplied an
    // overriding artifact for this fixture. A callback that does neither wrote
    // nothing, and a case resting on it is asserting over the baseline.
    const before = JSON.stringify(register);
    const extra = mutate(register) || {};
    if (JSON.stringify(register) !== before || Object.keys(extra).length) MUTATIONS_PLANTED += 1;
    for (const directory of [
      "closure/typescript-mapper",
      "contracts",
      "decisions",
      "charters",
      "schemas",
      "tools",
      "crates",
      "crates/fixture-owned/src",
      "authority/dag",
      "authority/state",
    ])
      fs.mkdirSync(path.join(root, directory), { recursive: true });
    fs.writeFileSync(
      path.join(root, "tools", "fixture-gamma.mjs"),
      extra.gammaText ?? "// fixture verdict producer\n",
    );
    // A module that only forwards. It is what makes the import walk's depth an
    // observable property rather than an implementation note.
    fs.writeFileSync(
      path.join(root, "tools", "fixture-shim.mjs"),
      extra.shimText ?? "// fixture re-export\n",
    );
    // The production surface a fixture charter declares, so an owed obligation
    // has somewhere real to be owed AT.
    fs.writeFileSync(path.join(root, "crates", "fixture-owned", "src", "lib.rs"), "// owned\n");
    // The two scripts a lane's own selection can be resolved THROUGH: one
    // computes a run-time predicate, the other enumerates the universe its
    // runner iterates. Both are executed by the validator, so a control can
    // move what they print and watch the resolution follow.
    // The predicate script is an ENTRY that reads its value out of a module
    // beside it, which is the shape the live lane selector has: the excluded
    // package set and the name-scoped selectors are declared one import away
    // from the script the lane names. Citing the entry alone would leave that
    // module outside the instrument's own trigger paths.
    fs.writeFileSync(
      path.join(root, "tools", "fixture-filter-internals.mjs"),
      `${extra.laneFilterImports ? `import ${JSON.stringify(extra.laneFilterImports)};\n` : ""}export const LANE_FILTER = ${JSON.stringify(extra.laneFilter ?? "not package(unrelated-crate)")};\n`,
    );
    fs.writeFileSync(
      path.join(root, "tools", "fixture-filter.mjs"),
      'import { LANE_FILTER } from "./fixture-filter-internals.mjs";\n\nprocess.stdout.write(LANE_FILTER);\n',
    );
    fs.writeFileSync(
      path.join(root, "tools", "fixture-owners-internals.mjs"),
      `${extra.laneOwnersImports ? `import ${JSON.stringify(extra.laneOwnersImports)};\n` : ""}export const OWNERS = ${JSON.stringify(extra.laneOwners ?? ["alpha", "beta"])};\n`,
    );
    fs.writeFileSync(
      path.join(root, "tools", "fixture-owners.mjs"),
      'import { OWNERS } from "./fixture-owners-internals.mjs";\n\nprocess.stdout.write(OWNERS.join("\\n"));\n',
    );
    fs.writeFileSync(path.join(root, FIXTURE_SUBJECT), extra.subject ?? FIXTURE_SUBJECT_TEXT);
    // Crates a `cargo` record may select. A package selector is the only way a
    // cargo record names the artifact its run is the verdict for, so the cargo
    // half of the acyclicity rule cannot be exercised without one.
    for (const name of extra.crates ?? []) {
      fs.mkdirSync(path.join(root, "crates", name, "src"), { recursive: true });
      fs.writeFileSync(
        path.join(root, "crates", name, "Cargo.toml"),
        `[package]\nname = "${name}"\n`,
      );
      fs.writeFileSync(path.join(root, "crates", name, "src", "lib.rs"), "// fixture crate\n");
    }

    fs.copyFileSync(
      path.join(PACKAGE_ROOT, "schemas", "closure-register.schema.json"),
      path.join(root, "schemas", "closure-register.schema.json"),
    );
    fs.writeFileSync(
      path.join(root, "contracts", "fixture.md"),
      extra.contract ?? FIXTURE_CONTRACT,
    );
    fs.writeFileSync(path.join(root, "decisions", "fixture.md"), "# Fixture decision\n");
    // Every charter narrates the same displaced owner the register declares,
    // because the validator resolves the two against each other. A control that
    // wants to move that name moves it in both places at once; one that wants
    // them to disagree moves it in one.
    const narrated = { displaced: extra.displacedOwner ?? FIXTURE_DISPLACED_OWNER };
    fs.writeFileSync(
      path.join(root, "charters", "TCM0R.md"),
      charter("TCM0R", 4, FIXTURE_OWNER, narrated),
    );
    // A charter the DAG names but the tree does not carry. The node still
    // declares its path, so the reader meets a citation with nothing behind it.
    if (extra.omitCharter !== "TCM1")
      fs.writeFileSync(
        path.join(root, "charters", "TCM1.md"),
        charter("TCM1", 4, "downstreamOwner" in extra ? extra.downstreamOwner : FIXTURE_OWNER, {
          ...narrated,
          ...(extra.downstreamSurfaces ? { surfaces: extra.downstreamSurfaces } : {}),
          ...(extra.downstreamNarrative ?? {}),
        }),
      );
    // A historical identity wrapper in the same train. It records that a
    // rejected node existed and delivers no outcome, so it narrates no owner
    // pair — which is what makes the narrative obligation's exemption an
    // exercised branch rather than an assumed one.
    fs.writeFileSync(
      path.join(root, "charters", "HIST.md"),
      charter("HIST", 4, FIXTURE_OWNER, { role: "history" }),
    );
    fs.writeFileSync(
      path.join(root, "charters", "UNREL.md"),
      charter("UNREL", 4, FIXTURE_OWNER, narrated),
    );
    fs.writeFileSync(
      path.join(root, "charters", "OTHER.md"),
      charter("OTHER", 4, FIXTURE_OWNER, narrated),
    );
    fs.writeFileSync(
      path.join(root, "authority", "root.toml"),
      toToml({
        schema: 5,
        implemented_ledger: "state/implemented.toml",
        modules: ["dag/fixture.toml"],
      }),
    );
    fs.writeFileSync(
      path.join(root, "authority", "dag", "fixture.toml"),
      toToml({
        schema: 4,
        module: "fixture",
        node: [
          {
            id: "TCM0R",
            predecessors: [],
            train: "rev11.typescript-mapper",
            semantic_role: "delivery",
            charter: "charters/TCM0R.md",
          },
          {
            id: "TCM1",
            predecessors: ["TCM0R"],
            train: "rev11.typescript-mapper",
            semantic_role: "delivery",
            charter: "charters/TCM1.md",
          },
          // A historical wrapper in the raising train: same owner header, no
          // outcome to narrate.
          {
            id: "HIST",
            predecessors: [],
            train: "rev11.typescript-mapper",
            semantic_role: "history",
            charter: "charters/HIST.md",
          },
          {
            id: "UNREL",
            predecessors: [],
            train: "other.train",
            semantic_role: "delivery",
            charter: "charters/UNREL.md",
          },
          // A strict descendant in a DIFFERENT train. Existence as a descendant
          // is not authority over this train's remainders, and this is the node
          // that proves the train bound is load-bearing rather than implied by
          // the descendant check.
          {
            id: "OTHER",
            predecessors: ["TCM0R"],
            train: "other.train",
            semantic_role: "delivery",
            charter: "charters/OTHER.md",
          },
        ],
      }),
    );
    fs.writeFileSync(
      path.join(root, "authority", "state", "implemented.toml"),
      'schema = 2\n\n[implementation]\n"TCM0R" = { status = "pending" }\n"TCM1" = { status = "pending" }\n"HIST" = { status = "pending" }\n"UNREL" = { status = "pending" }\n"OTHER" = { status = "pending" }\n',
    );
    fs.writeFileSync(
      path.join(root, "closure", "typescript-mapper", "register.toml"),
      toToml(register),
    );
    fs.mkdirSync(path.join(root, ".github", "workflows"), { recursive: true });
    // The workflow is a validator INPUT, not scenery: every lane resolution and
    // every trigger-path measurement reads it. A tree without one is therefore a
    // tree this instrument cannot resolve, rather than one with nothing to say.
    if (extra.workflow !== null)
      fs.writeFileSync(
        path.join(root, ".github", "workflows", "ci.yml"),
        extra.workflow === undefined ? FIXTURE_WORKFLOW : extra.workflow,
      );
    if (extra.view !== undefined) fs.writeFileSync(path.join(root, VIEW_RELATIVE), extra.view);

    FIXTURE_ROOTS.push(root);
    // The pinned universe is the baseline's, never the mutated register's, and
    // fixture/subject paths resolve inside the fixture tree.
    return {
      root,
      ...analyze(root, {
        universe: extra.universe ?? FIXTURE_UNIVERSE,
        topology: extra.topology ?? FIXTURE_TOPOLOGY,
        exercising: extra.exercising ?? {},
        repoRoot: root,
      }),
    };
  } catch (error) {
    fs.rmSync(root, { recursive: true, force: true });
    throw error;
  }
}

// The four channels a refusal arrives through. Each derives the refusal from
// the model rather than recording that the case believes it saw one, and each
// counts it, so `control` can require that a planted mutation was actually
// rejected instead of merely planted.

/** Assert at least one validator error contains `fragment`. */
function refuses({ errors }, fragment) {
  assert.ok(
    errors.some((error) => error.includes(fragment)),
    `expected an error containing ${JSON.stringify(fragment)}; got:\n${errors.join("\n") || "(none)"}`,
  );
  REFUSALS_OBSERVED += 1;
}

/** Assert a record carries a DERIVED limit containing `fragment`. */
function limits({ model }, proofId, fragment) {
  const declared = model.proofLimits.get(proofId) ?? [];
  assert.ok(
    declared.some((limit) => limit.includes(fragment)),
    `expected a limit on ${proofId} containing ${JSON.stringify(fragment)}; got:\n${declared.join("\n") || "(none)"}`,
  );
  REFUSALS_OBSERVED += 1;
}

/** Assert the tool declines to certify this derivation. */
function refusesCertification(result, pattern) {
  const certified = certification(result);
  assert.equal(certified.ok, false, "this derivation must not be certifiable");
  assert.match(certified.reason, pattern);
  REFUSALS_OBSERVED += 1;
}

/** Assert the generated view no longer reads as fresh against its register. */
function refusesStaleView(root, model) {
  assert.equal(viewIsFresh(root, model), false, "a superseded view must not read as fresh");
  REFUSALS_OBSERVED += 1;
}

const claimStatus = (model, id) => model.claimStatus.get(id).status;

/**
 * The pinned universe with the anchors recomputed from a mutated register.
 *
 * The anchor pin has its own control. A case that moves an anchor for some
 * OTHER reason — pointing an atom at the artifact its rewritten fixture list
 * reaches, declaring a surface an obligation is owed at — would otherwise be
 * refused by that pin before the property it is actually testing is ever
 * reached, and a refusal for the wrong reason discriminates nothing.
 */
const withAnchors = (register, extra = {}) => ({
  universe: {
    ...FIXTURE_UNIVERSE,
    anchors: Object.fromEntries(
      register.atom.map((atom) => [`atom:${atom.id}`, anchorDigest(atom)]),
    ),
  },
  ...extra,
});

/**
 * The pinned universe extended with a record's skip basis.
 *
 * A skip basis is a pinned proposition and exists only on a record that
 * declares a skip count, which the baseline does not. A case that gives one a
 * skip count is therefore ADDING a proposition, and declares the pin for it
 * rather than being refused by the universe check before the property it is
 * actually testing is reached.
 */
const withSkipBasis = (proofId, basis, extra = {}) => ({
  universe: {
    ...FIXTURE_UNIVERSE,
    statements: {
      ...FIXTURE_UNIVERSE.statements,
      [`proof:${proofId}.skip_basis`]: statementDigest(basis),
    },
  },
  ...extra,
});

/** Turn the fixture's one adapter into an external cargo runner. */
function asCargo(register, overrides = {}) {
  Object.assign(register.adapter[0], {
    runner: "cargo",
    reexecution: "external",
    summary_grammar: "libtest",
    refresh_job: FIXTURE_REFRESH_JOB,
    refresh_filter: "rust",
    refresh_command: FIXTURE_REFRESH_COMMAND,
    ...overrides,
  });
  for (const proof of register.proof) {
    delete proof.count_key;
    proof.terminal_summary = `test result: ok. ${proof.passed} passed; 0 failed; 0 ignored; 0 measured; 0 filtered out`;
  }
}

// --- the baseline must be clean, or every control below proves nothing ------

selfAssertion("the fixture baseline validates and derives a reviewable state", () => {
  const { errors, model } = run();
  assert.deepEqual(errors, []);
  assert.equal(model.state, READY_FOR_REVIEW);
  assert.equal(claimStatus(model, "CLM-ALPHA"), PROVEN);
  assert.equal(claimStatus(model, "CLM-BETA"), PROVEN_BOUNDED);
  assert.equal(claimStatus(model, "CLM-GAMMA"), PROVEN);
  for (const [, derived] of model.claimStatus) assert.equal(derived.admissible, true);
  for (const [, limits] of model.proofLimits) assert.deepEqual(limits, []);
  assert.equal(certification({ errors, model }).ok, true);
});

// --- the twelve mandatory failure classes -----------------------------------

control("an omitted, added, or misrouted claim, atom, row, or finding is refused", () => {
  covers("omitted-claim");

  // The universe the register is measured against is pinned outside it, so a
  // deletion fails instead of quietly shrinking the set the register defines
  // for itself. That is the difference between a closed universe and a list.
  refuses(
    run((register) => {
      register.claim = register.claim.filter((claim) => claim.id !== "CLM-GAMMA");
      register.atom = register.atom.filter((atom) => atom.claim !== "CLM-GAMMA");
      register.proof = register.proof.filter((proof) => proof.id !== "P-gamma");
      register.control = register.control.filter((row) => row.id !== "CTL-gamma");
    }),
    'claim universe: "CLM-GAMMA" omitted from the register',
  );

  refuses(
    run((register) => {
      register.atom = register.atom.filter((atom) => atom.id !== "A-plain");
      register.proof[0].covers = ["A-stated"];
      register.finding = register.finding.map((finding) =>
        finding.atom ? { ...finding, atom: "A-stated" } : finding,
      );
    }),
    'atom universe of CLM-ALPHA: "A-plain" omitted from the register',
  );

  refuses(
    run((register) => {
      register.atom.push({
        id: "A-invented",
        claim: "CLM-ALPHA",
        statement: "An obligation nobody pinned.",
        evidence_anchor: "contracts/fixture.md",
        shipped_obligation: "met",
      });
    }),
    'atom universe of CLM-ALPHA: "A-invented" is not in the pinned universe',
  );

  // Moving an atom between claims is neither an addition nor a removal from
  // the register's own point of view, which is exactly why the pin is per
  // claim rather than a flat atom list.
  refuses(
    run((register) => {
      register.atom.find((atom) => atom.id === "A-beta").claim = "CLM-ALPHA";
    }),
    'atom universe of CLM-BETA: "A-beta" omitted from the register',
  );

  refuses(
    run((register) => {
      register.row = register.row.filter((row) => row.kind !== "deletion");
    }),
    'deletion rows: "Fixture displaced route" omitted from the register',
  );

  refuses(
    run((register) => {
      register.finding = register.finding.filter((finding) => finding.id !== "AR7");
    }),
    "finding AR7: omitted from the register",
  );

  refuses(
    run((register) => {
      register.finding.push({
        id: "AR99",
        statement: "Invented.",
        claim: "CLM-ALPHA",
        atom: "A-plain",
      });
    }),
    "finding AR99: not in the closed finding universe",
  );

  refuses(
    run((register) => {
      const remainder = register.finding.find((finding) => finding.id === "C2");
      delete remainder.residue;
      remainder.claim = "CLM-ALPHA";
      remainder.atom = "A-plain";
    }),
    "finding C2: is a remainder entry and may not close against a claim",
  );

  refuses(
    run((register) => {
      register.finding.find((finding) => finding.id === "C7").residue = ALLOWED_RESIDUES[0];
    }),
    "expected TCM0-R-TOPOLOGY-SELECTION",
  );

  // Both routes at once, and neither. The schema requires neither field and
  // excludes neither, so a finding carrying a claim AND a residue is
  // expressible: it would be counted as closed against the claim and carried as
  // a remainder, which is the reconciliation reading two answers out of one row.
  refuses(
    run((register) => {
      register.finding.find((finding) => finding.id === "C2").claim = "CLM-ALPHA";
    }),
    "finding C2: must route to exactly one claim or residue",
  );
  refuses(
    run((register) => {
      const entry = register.finding.find((finding) => finding.id === "AR7");
      delete entry.claim;
      delete entry.atom;
    }),
    "finding AR7: must route to exactly one claim or residue",
  );

  // An atom routed at a claim that does not exist belongs to no claim summary,
  // so the obligation it states is counted nowhere.
  refuses(
    run((register) => {
      register.atom[1].claim = "CLM-ABSENT";
    }),
    "unknown claim CLM-ABSENT",
  );

  // Both row kinds are required. A register that names what it deleted and
  // never names what survived tells half the migration story.
  refuses(
    run((register) => {
      register.row = register.row.filter((row) => row.kind !== "survivor");
    }),
    "rows: no survivor row",
  );

  // A deletion or survivor row hands its subject to an acceptance criterion.
  // Something that is not an acceptance id names no criterion to resolve.
  refuses(
    run((register) => {
      register.row[0].receiving_criterion = "TCM1_AC1";
    }),
    "is not an acceptance id",
  );
});

control("the tool refuses to certify a derived state that is not reviewable", () => {
  covers("omitted-claim");

  // One ordinary edit: an atom id dropped from a proof's coverage. The atom is
  // still in the pinned universe, so no universe check fires and no error is
  // produced — the claim simply derives OPEN. A tool that gated only on its
  // error list would print a pass line here, and that line is itself
  // transcribable as an evidence record.
  const { errors, model } = run((register) => {
    register.proof[0].covers = ["A-stated"];
  });
  assert.deepEqual(errors, [], "the gap this closes produces no error of its own");
  assert.equal(claimStatus(model, "CLM-ALPHA"), OPEN);
  assert.deepEqual(model.claimStatus.get("CLM-ALPHA").uncovered, ["A-plain"]);
  assert.equal(model.state, OPEN);

  refusesCertification({ errors, model }, /OPEN/u);

  const clean = run();
  assert.equal(certification(clean).ok, true);
});

control("an author-set status anywhere in the input is a schema error", () => {
  covers("forbidden-input-status");

  refuses(
    run((register) => {
      register.claim[0].status = PROVEN;
    }),
    "additional property status",
  );

  refuses(
    run((register) => {
      register.finding[0].status = PROVEN;
    }),
    "additional property status",
  );

  refuses(
    run((register) => {
      register.residue[0].status = PROVEN;
    }),
    "additional property status",
  );

  // A limit is derived from an unresolved refresh binding, never volunteered.
  // Leaving the field authorable is what made disclosing one optional, and
  // omitting one free.
  refuses(
    run((register) => {
      register.proof[0].limits = ["sampled two of four inputs"];
    }),
    "additional property limits",
  );

  // The closed shape is closed EVERYWHERE, not only where a status was once
  // written. Every row kind the register carries is planted with a property
  // its schema does not declare, so relaxing any one of them fails here
  // rather than only on the object somebody happened to control.
  for (const collection of [
    "adapter",
    "claim",
    "atom",
    "proof",
    "control",
    "finding",
    "residue",
    "receiving",
    "row",
    "transfer",
  ])
    refuses(
      run((register) => {
        register[collection][0].undeclared_property = "anything";
      }),
      `${collection}[0]: additional property undeclared_property`,
    );

  // And the two objects that are not collections: the envelope itself and the
  // ratification block a reader takes the owner from.
  refuses(
    run((register) => {
      register.undeclared_property = "anything";
    }),
    "additional property undeclared_property",
  );
  refuses(
    run((register) => {
      register.ratification.undeclared_property = "anything";
    }),
    "additional property undeclared_property",
  );
});

control("a removed remainder, a removed receiving row, or an unauthorized owner is refused", () => {
  covers("removed-residue-owner");

  refuses(
    run((register) => {
      register.residue = register.residue.filter((residue) => residue.id !== ALLOWED_RESIDUES[1]);
      register.receiving = register.receiving.filter((row) => row.residue !== ALLOWED_RESIDUES[1]);
      register.transfer = register.transfer.filter((row) => row.residue !== ALLOWED_RESIDUES[1]);
      register.finding.find((finding) => finding.id === "C7").residue = ALLOWED_RESIDUES[0];
    }),
    `residue ${ALLOWED_RESIDUES[1]}: missing from the register`,
  );

  refuses(
    run((register) => {
      register.receiving = register.receiving.filter((row) => row.residue !== ALLOWED_RESIDUES[2]);
    }),
    `residue ${ALLOWED_RESIDUES[2]}: no receiving criterion`,
  );

  // Removal is only half of a closed remainder vocabulary, and it is the
  // easier half. Adding a remainder the ruling never allowed is the direction
  // that matters: an unproven atom then leaves through a route nobody
  // authorized, while every row already in the register stays exactly where it
  // was and every count still reconciles. Nothing else refuses this — the
  // remainder ids are not diffed against a pinned universe the way claims,
  // atoms, statements and anchors are, so this single check is the whole rail.
  refuses(
    run((register) => {
      register.residue.push({
        id: "TCM0-R-INVENTED",
        statement: "A remainder no ruling allows.",
      });
    }),
    "residue TCM0-R-INVENTED: not an admissible residue",
  );

  // Deleting the residue row ALONE, leaving its findings routed to it. The
  // diagnostic must still be produced: an exception here would abort the whole
  // analysis and report a stack trace instead of the error it had computed.
  const bare = run((register) => {
    register.residue = register.residue.filter((residue) => residue.id !== ALLOWED_RESIDUES[1]);
  });
  refuses(bare, `residue ${ALLOWED_RESIDUES[1]}: missing from the register`);
  refuses(bare, `finding C7: residue ${ALLOWED_RESIDUES[1]} has no register row`);

  refuses(
    run((register) => {
      register.receiving[0].owner_node = "UNREL";
      register.receiving[0].criterion = "UNREL-AC1";
    }),
    "is not a strict descendant of TCM0R",
  );

  // A strict descendant is not automatically an authorized owner. `OTHER`
  // descends from the raising node but belongs to another train, and every
  // charter in the program declares the same four ordinals, so without the
  // train bound an unrelated vertical's block would resolve cleanly here.
  refuses(
    run((register) => {
      register.receiving[0].owner_node = "OTHER";
      register.receiving[0].criterion = "OTHER-AC1";
      register.receiving[0].gate = "OTHER acceptance: fixture gate.";
    }),
    "criterion owner OTHER is outside the rev11.typescript-mapper train",
  );

  refuses(
    run((register) => {
      register.receiving[0].criterion = "TCM1-AC9";
    }),
    "criterion TCM1-AC9 is not declared by TCM1",
  );

  // The ordinals are interchangeable boilerplate; the ROLE is not. Rebinding a
  // sole-owner obligation onto the self-waivable bounded-work slot is the
  // misroute the bare identifier cannot see.
  refuses(
    run((register) => {
      register.receiving[0].criterion = "TCM1-AC4";
    }),
    'TCM1 declares TCM1-AC4 as "bounded work", not "sole-owner outcome"',
  );

  refuses(
    run((register) => {
      register.atom.find((atom) => atom.id === "A-stated").received_by = "TCM1-AC3";
    }),
    'TCM1 declares TCM1-AC3 as "incremental equivalence", not "sole-owner outcome"',
  );

  refuses(
    run((register) => {
      register.row[0].receiving_criterion = "TCM0R-AC2";
    }),
    'TCM0R declares TCM0R-AC2 as "positive contract", not "sole-owner outcome"',
  );

  // The gate is the sentence the generated view publishes. It must name the
  // owner the criterion was actually resolved against, or a misrouted row reads
  // as an obligation on a block that never took it.
  refuses(
    run((register) => {
      register.receiving[0].gate = "TCM4 acceptance: a block that does not own this row.";
    }),
    'gate must open with "TCM1 acceptance:"',
  );

  refuses(
    run((register) => {
      register.receiving[0].order = 3;
    }),
    "receiving order is not 1..n",
  );

  refuses(
    run((register) => {
      register.transfer.push({
        atom: "A-hang",
        residue: ALLOWED_RESIDUES[1],
        approved_by: "charters/TCM0R.md",
      });
    }),
    "transfer: duplicate atom A-hang",
  );

  // A downstream charter still handing the capability to the displaced owner
  // is the same defect in narrative form, and a charter says this twice. The
  // generated header is one statement; the outcome sentence a reader meets is
  // the other, and they can disagree — so each is resolved on its own.
  refuses(
    run(() => ({ downstreamOwner: FIXTURE_DISPLACED_OWNER })),
    "charter TCM1: declares owner",
  );

  // A charter with no generated header at all. The prose can still read as
  // correct while the field a reader's tooling resolves is simply missing, so
  // an absent header fails rather than being treated as an unstated agreement.
  refuses(
    run(() => ({ downstreamOwner: null })),
    "charter TCM1: declares no owner header",
  );

  // A charter the DAG cites and the tree does not carry: the citation resolves
  // to nothing, so the owner it was supposed to state is unread rather than
  // wrong, and silence there would read as agreement.
  refuses(
    run(() => ({ omitCharter: "TCM1" })),
    "charter TCM1: ENOENT",
  );

  // A regenerated header over untouched prose: the header now names the
  // ratified owner while the sentence still ends the migration at the
  // displaced one. Reading the header alone would pass this.
  refuses(
    run(() => ({ downstreamNarrative: { final: FIXTURE_DISPLACED_OWNER } })),
    "charter TCM1: narrates the final and sole owner as",
  );

  // The other half of the pair. A charter that renames what it displaces is
  // no longer describing this register's migration, so the two artifacts have
  // stopped agreeing about which capability moved.
  refuses(
    run(() => ({ downstreamNarrative: { displaced: "fixture:some other owner" } })),
    "charter TCM1: narrates the displaced owner as",
  );

  // Deleting the sentence is not a way to satisfy it. A delivery charter that
  // states no pair states nothing a reader can check, which is the silence
  // the header-only check used to accept.
  refuses(
    run(() => ({ downstreamNarrative: { role: "history" } })),
    "charter TCM1: its outcome narrative states no current/final owner pair",
  );

  // The pair is only the charter's statement while there is exactly one of it.
  // A compliant sentence written above a stale one leaves a reader meeting
  // both, so resolving the first match — anywhere in the document, or anywhere
  // in the section — reports a charter clean that a reader would not.
  refuses(
    run(() => ({
      downstreamNarrative: {
        extraOutcome: `\nSuperseded note. The current owner is **${FIXTURE_DISPLACED_OWNER}**. The final and sole owner is **${FIXTURE_DISPLACED_OWNER}**.\n`,
      },
    })),
    "states more than one current/final owner pair",
  );
  refuses(
    run(() => ({
      downstreamNarrative: {
        extraOutcome: `\n## Independently acceptable outcome\n\nStale. The current owner is **${FIXTURE_DISPLACED_OWNER}**. The final and sole owner is **${FIXTURE_DISPLACED_OWNER}**.\n`,
      },
    })),
    'declares two "Independently acceptable outcome" sections',
  );

  // And the sentence must live in the section that owns it: a compliant pair
  // written anywhere else in the charter is not the outcome narrative.
  refuses(
    run(() => ({
      downstreamNarrative: {
        role: "history",
        extraOutcome: `\n## Notes\n\nThe current owner is **${FIXTURE_DISPLACED_OWNER}**. The final and sole owner is **${ownerCapability(FIXTURE_OWNER)}**.\n`,
      },
    })),
    "charter TCM1: its outcome narrative states no current/final owner pair",
  );

  // A receiving row filed under a remainder the register does not carry hands
  // the obligation to a sequence that exists nowhere.
  refuses(
    run((register) => {
      register.receiving[0].residue = "TCM0-R-NOT-A-RESIDUE";
    }),
    "unknown residue",
  );

  // An owner that is not a node of the DAG at all. Descendancy is checked
  // AFTER existence, so without this the row would be measured against a
  // graph position nothing occupies.
  refuses(
    run((register) => {
      register.receiving[0].owner_node = "ZZZZ";
    }),
    "is not a DAG node",
  );

  // A criterion belonging to some other block. An ordinal is interchangeable
  // across charters, so the row has to name a criterion its own owner owns.
  refuses(
    run((register) => {
      register.receiving[0].criterion = "OTHER-AC1";
    }),
    "is not owned by TCM1",
  );

  // A transfer of an atom the register does not declare: the approval carries
  // an obligation nothing states.
  refuses(
    run((register) => {
      register.transfer[0].atom = "A-not-an-atom";
    }),
    "unknown atom",
  );
});

control("a dependency the run did not have is refused", () => {
  covers("missing-dependency");

  refuses(
    run((register) => {
      register.atom[0].contract_section = "9. Absent";
    }),
    "contract section not found: 9. Absent",
  );

  refuses(
    run((register) => {
      register.ratification.decision = "decisions/never-written.md";
    }),
    "ratification decision",
  );

  refuses(
    run((register) => {
      register.proof[0].control = "CTL-absent";
    }),
    "unknown control CTL-absent",
  );

  // A stated contract obligation with nothing downstream to enforce it is the
  // same defect in authority form: the dependency that would make it real is
  // missing.
  refuses(
    run((register) => {
      delete register.atom[0].received_by;
    }),
    "must name the criterion that enforces it",
  );

  refuses(
    run((register) => {
      register.atom[0].received_by = "TCM1-AC9";
    }),
    "criterion TCM1-AC9 is not declared by TCM1",
  );

  refuses(
    run((register) => {
      register.atom[0].received_by = "UNREL-AC1";
    }),
    "neither TCM0R nor a strict descendant",
  );

  // A named fixture that is not there is the same defect in evidence form: the
  // record cites an input its run could not have read.
  refuses(
    run((register) => {
      register.proof[0].fixtures = ["contracts/renamed-away.md"];
    }),
    "proof P-alpha: fixture contracts/renamed-away.md does not resolve",
  );

  refuses(
    run((register) => {
      register.claim[0].subject = ["contracts/deleted.md"];
    }),
    "claim CLM-ALPHA: subject contracts/deleted.md does not resolve",
  );

  // An evidence anchor is a dependency too: an atom pointing at an artifact
  // that is not there could never have its relevance resolved.
  refuses(
    run((register) => {
      register.atom[1].evidence_anchor = "contracts/never-written.md";
    }),
    "evidence anchor does not resolve",
  );

  refuses(
    run((register) => {
      register.atom[1].evidence_anchor = "contracts";
    }),
    "is a whole top-level tree",
  );
});

control("a contract section that exists but no longer states the obligation is refused", () => {
  covers("missing-dependency");

  // The heading survives; the body is gutted. A binding that only checks for a
  // heading reports the atom as proven from a title.
  refuses(
    run(() => ({
      contract: FIXTURE_CONTRACT.replace(
        `The stated obligation: ${STATED_ANCHOR}, with a body so the section is not empty.`,
        "TBD.",
      ),
    })),
    "no longer states",
  );

  refuses(
    run((register) => {
      delete register.atom.find((atom) => atom.id === "A-stated").contract_anchor;
    }),
    "must quote the text it relies on",
  );

  // Two sections with one title would otherwise collapse to the later body, so
  // an atom bound to the first would be validated against the second.
  refuses(
    run(() => ({ contract: `${FIXTURE_CONTRACT}\n## 1. Stated\n\nA second body.\n` })),
    'duplicate section heading "1. Stated"',
  );

  // A quotation with no section names no place to look for itself.
  refuses(
    run((register) => {
      delete register.atom[0].contract_section;
    }),
    "a contract anchor without a contract section quotes nothing",
  );
});

control("a generated view that no longer matches its register is stale", () => {
  covers("stale-evidence");

  const baseline = run();
  const view = renderView(baseline.model);

  const fresh = run(() => ({ view }));
  assert.deepEqual(fresh.errors, []);
  assert.equal(
    viewIsFresh(fresh.root, fresh.model),
    true,
    "the freshly rendered view must be fresh",
  );

  const stale = run((register) => {
    // A field the view renders that the statement pin does not hold, so this
    // control measures view freshness rather than tripping that pin first. The
    // charters narrate the same name, so it moves in both places at once —
    // otherwise the owner resolution would refuse the register before freshness
    // was ever measured, and this control would be proving the wrong thing.
    const restated = "fixture:displaced owner, restated after the view was written";
    register.ratification.displaced_owner = restated;
    return { view, displacedOwner: restated };
  });
  assert.deepEqual(stale.errors, []);
  refusesStaleView(stale.root, stale.model);

  const absent = run();
  assert.equal(viewIsFresh(absent.root, absent.model), false, "a missing view is not a fresh view");

  // The other half of the same property: a run that refused the register may
  // not leave a warm artifact behind. Without this the view on disk could be
  // written from a partial reading of a register the same run rejected, and the
  // freshness comparison above would then be measuring one degraded derivation
  // against another.
  assert.equal(
    publication(baseline).publish,
    true,
    "a clean derivation is what the generated view is written from",
  );
  const refused = run((register) => {
    register.atom[0].evidence_anchor = "tools/never-written.mjs";
  });
  assert.ok(refused.errors.length, "the mutation must refuse the register");
  assert.equal(
    publication(refused).publish,
    false,
    "a refused derivation must not be published as the generated view",
  );
  assert.equal(
    publication({ errors: [], model: null }).publish,
    false,
    "a run that derived nothing must not be published either",
  );
});

// A transcribed counter is only as good as the last time somebody ran the
// command, and view freshness is a different property entirely. Every live
// record whose runner is `node` is re-executed here and its five counts are
// re-derived from the live output, so a drifted count fails rather than reading
// as current. Comparing derived counts rather than substrings is what stops a
// grown count whose old value is a prefix of the new one — 26 inside 260 — from
// still matching. The suite excludes only itself, because re-entering this file
// would not terminate; that record is bound instead by the declared-case count
// below.
selfAssertion("every re-derivable evidence record is re-executed against its transcription", () => {
  covers("stale-evidence");

  const { model } = analyze(PACKAGE_ROOT);
  const adapters = new Map(model.register.adapter.map((row) => [row.id, row]));
  const repoRoot = path.resolve(PACKAGE_ROOT, "..", "..");
  const selfCommand = path.relative(repoRoot, fileURLToPath(import.meta.url)).replaceAll("\\", "/");

  const expected = model.register.proof.filter(
    (proof) =>
      adapters.get(proof.adapter).reexecution === "instrument" &&
      ![...adapters.get(proof.adapter).argv_prefix, ...proof.argv_tail].includes(selfCommand),
  ).length;

  let reExecuted = 0;
  for (const proof of model.register.proof) {
    const adapter = adapters.get(proof.adapter);
    if (adapter.reexecution !== "instrument") continue;
    const argv = [...adapter.argv_prefix, ...proof.argv_tail];
    if (argv.includes(selfCommand)) continue;

    // `node --test` refuses to start a nested run when it sees the parent
    // runner's context, so the child would report "skipping running files"
    // instead of executing. Hand it a clean environment.
    const env = { ...process.env };
    delete env.NODE_TEST_CONTEXT;
    const result = spawnSync(process.execPath, argv, {
      cwd: repoRoot,
      encoding: "utf8",
      env,
      timeout: INSTRUMENT_LANE_COMMAND_DEADLINE_MS,
    });
    assert.equal(
      result.status,
      0,
      `${proof.id}: ${argv.join(" ")} exited ${result.status}\n${result.stderr}`,
    );
    const output = `${result.stdout}${result.stderr}`.replaceAll("\r\n", "\n");
    const grammar = adapter.summary_grammar;
    const live = parseTerminalSummary(grammar, output, proof.count_key);
    assert.ok(live, `${proof.id}: the live run emitted no ${grammar} terminal summary:\n${output}`);
    assert.deepEqual(
      live,
      parseTerminalSummary(grammar, proof.terminal_summary, proof.count_key),
      `${proof.id}: the live counts are no longer the ones this record transcribes`,
    );
    // The counts are the load-bearing half, but a tool line also carries fields
    // that are not counts. Those are compared as recorded.
    for (const fragment of proof.terminal_summary.split(" | "))
      assert.ok(
        output.includes(fragment.trim()),
        `${proof.id}: the recorded terminal summary is no longer what the command emits.\nrecorded: ${fragment.trim()}\nemitted:\n${output}`,
      );
    reExecuted += 1;
  }
  // An exact count, not a floor: a record added under an instrument adapter
  // without being re-executed here would otherwise pass on the strength of the
  // records that already were.
  assert.equal(
    reExecuted,
    expected,
    `every instrument-re-derivable record must re-execute; ran ${reExecuted} of ${expected}`,
  );
  assert.ok(expected >= 2, "the live register declares no re-derivable records");
});

// A repository mirror the drivable controls are re-applied to: the package
// tree copied whole, plus every artifact outside it that the validator resolves
// — the workflow it reads, the fixtures and anchors it stats, the manifests of
// the packages the records select, and the one fixture directory a record's
// count is re-derived from, which is copied with its contents because an empty
// directory would answer that count wrongly. Everything else is absent on
// purpose: a mutation belongs in a copy, never in the tree under review, where
// an interrupted run would leave it behind as a real edit.
function mirrorTree(model, repoRoot) {
  const adapters = new Map(model.register.adapter.map((row) => [row.id, row]));
  const root = fs.mkdtempSync(path.join(os.tmpdir(), "closure-mirror-"));
  FIXTURE_ROOTS.push(root);
  const clone = (relative) => {
    const destination = path.join(root, relative);
    if (fs.existsSync(destination)) return;
    fs.mkdirSync(path.dirname(destination), { recursive: true });
    fs.cpSync(path.join(repoRoot, relative), destination, { recursive: true });
  };
  clone(path.relative(repoRoot, PACKAGE_ROOT).replaceAll("\\", "/"));
  clone(CI_WORKFLOW);
  clone("scripts");
  // Module resolution only, never a path the validator confines: a junction is
  // not copied and not walked. Link every installed tree, not only the
  // repository root — a record that resolves a workspace package's own
  // `node_modules` must see the same path the checkout has.
  linkInstalledModuleTrees(repoRoot, root);

  const counted = new Set();
  for (const proof of model.register.proof)
    if (adapters.get(proof.adapter).count_source === "fixture-directory")
      for (const fixture of proof.fixtures) counted.add(path.posix.dirname(fixture));

  const required = new Set([
    ...model.register.atom.map((atom) => atom.evidence_anchor),
    ...model.register.proof.flatMap((proof) => proof.fixtures),
    ...model.register.control.filter((row) => row.kind === "source").map((row) => row.subject),
    ...counted,
  ]);
  for (const proof of model.register.proof)
    for (const selected of selectedPackages(adapters.get(proof.adapter), proof))
      required.add(`crates/${selected}/Cargo.toml`);

  for (const relative of [...required].sort()) {
    const destination = path.join(root, relative);
    if (fs.existsSync(destination)) continue;
    const source = path.join(repoRoot, relative);
    if (fs.statSync(source).isDirectory() && !counted.has(relative))
      fs.mkdirSync(destination, { recursive: true });
    else clone(relative);
  }
  return root;
}

// A control's transcript describes a run somebody once did. The uniqueness and
// absence checks beside it establish that the mutation COULD have applied to
// this tree and is not sitting in it, which is what stops an invented
// transcript — but nothing there re-runs the command, so a control whose
// refusal has since stopped happening still reads as evidence.
//
// Every control bound to a record this instrument can invoke is therefore
// re-applied: the mutation goes into the mirror, the record's own command runs
// against it, and the refusal must be the runner's own, in the grammar its
// adapter declares, carrying the counts the control transcribes.
//
// The clean run BEFORE the mutation is what makes that refusal attributable.
// Without it a mirror that was already red would report every control as
// killed, which is the same false pass as a mutation that never applied.
//
// The lane split is a toolchain boundary, not a sample. This suite drives the
// controls whose bound record runs under `node` — every change to the roadmap
// tree re-applies those. The rest mutate an artifact too, and are driven by the
// control lane, which has the Rust toolchain their records need and which can
// spawn this suite without re-entering it. The partition itself is asserted
// separately, so a control cannot end up owned by neither.
selfAssertion(
  "every control this instrument can drive is re-applied to a mirror of the tree and refused",
  () => {
    const { model } = analyze(PACKAGE_ROOT);
    const repoRoot = path.resolve(PACKAGE_ROOT, "..", "..");
    const selfCommand = path
      .relative(repoRoot, fileURLToPath(import.meta.url))
      .replaceAll("\\", "/");

    const owned = controlsFor(model, INSTRUMENT_LANE, selfCommand);
    const mirror = mirrorTree(model, repoRoot);
    const env = { ...process.env };
    delete env.NODE_TEST_CONTEXT;
    const spawn = (argv) =>
      spawnSync(process.execPath, argv, {
        cwd: mirror,
        encoding: "utf8",
        env,
        timeout: INSTRUMENT_LANE_COMMAND_DEADLINE_MS,
      });

    let reApplied = 0;
    for (const control of owned) {
      reapply({ model, control, mirror, spawn });
      reApplied += 1;
    }

    // An exact count, not a floor: a control added under a record this lane
    // owns without being re-applied here would otherwise pass on the strength
    // of the controls that already were.
    assert.equal(
      reApplied,
      owned.length,
      `every control this lane owns must be re-applied; ran ${reApplied} of ${owned.length}`,
    );
    assert.ok(owned.length >= 2, "the live register declares no controls for this lane");
  },
  { timeout: INSTRUMENT_LANE_DEADLINE_MS },
);

// The partition is the part a lane cannot assert about itself. Either lane can
// re-apply every control it OWNS and still leave a control owned by neither,
// which is the sampling this instrument is required not to do — and the way it
// would happen is mundane: a control lands under a record whose runner the fast
// lane does not drive, and nothing notices that no other lane picked it up.
//
// So ownership is resolved here as a total partition over the register's own
// control universe, with NO transcribed remainder: a control that mutates no
// artifact is re-applied by the lane its record's runner decides like any
// other, because its record's counters and its delta's refusal are both claims
// only a run re-derives — an empty selection reports how many tests it failed
// to select, which is a property of the tree's current test inventory and not
// only of the runner. The lane this suite delegates to is resolved against the
// workflow rather than assumed to run: the job must exist, must issue that
// lane's complete command line, must install the toolchain its controls'
// records need — including a cargo subcommand that is not part of the
// toolchain — and must be gated on the trigger filter that covers both the
// lane's own entry and every subject it mutates. A delegated lane no job runs
// is a transcript with extra steps.
selfAssertion("every control is owned by a re-applying lane, and that lane runs in CI", () => {
  const { model } = analyze(PACKAGE_ROOT);
  const repoRoot = path.resolve(PACKAGE_ROOT, "..", "..");
  const selfCommand = path.relative(repoRoot, fileURLToPath(import.meta.url)).replaceAll("\\", "/");

  const lanes = new Map(
    model.register.control.map((control) => [control.id, laneFor(model, control, selfCommand)]),
  );
  const byLane = (lane) =>
    model.register.control.filter((control) => lanes.get(control.id) === lane).map((row) => row.id);
  const control = (id) => model.register.control.find((row) => row.id === id);

  // Total and disjoint over the register's own universe, with no remainder:
  // every control lands in exactly one of the two re-applying lanes, and a
  // control that mutates no artifact is among them — the runner its record
  // names decides which lane, exactly as for a control that edits a file.
  assert.deepEqual(
    [...byLane(INSTRUMENT_LANE), ...byLane(CONTROL_LANE)].sort(),
    model.register.control.map((row) => row.id).sort(),
    "a control escaped both lanes; every control, command-shaped included, must be re-applied by one of them",
  );
  for (const row of model.register.control.filter((row2) => row2.kind !== "source"))
    assert.ok(
      row.argv_delta?.length,
      `${row.id} mutates no artifact and adds no argument, so nothing about it is checkable`,
    );

  // Both driving lanes are non-empty, so neither can quietly become the whole
  // partition, and the lane this one delegates to has to be a real file.
  assert.ok(byLane(INSTRUMENT_LANE).length >= 1, "no control is driven by this suite");
  assert.ok(byLane(CONTROL_LANE).length >= 1, "no control is driven by the control lane");
  assert.ok(
    fs.existsSync(path.join(repoRoot, CONTROL_LANE_ENTRY)),
    `the delegated control lane ${CONTROL_LANE_ENTRY} does not exist`,
  );

  const workflow = fs.readFileSync(path.join(repoRoot, CI_WORKFLOW), "utf8");
  const hosting = [...workflowJobs(workflow)].filter(([, body]) =>
    laneCommandLine(body, CONTROL_LANE_COMMAND),
  );
  assert.equal(
    hosting.length,
    1,
    `exactly one job must issue the control lane; ${hosting.length} do`,
  );
  const [jobName, body] = hosting[0];
  assert.equal(
    laneCommandLine(body, CONTROL_LANE_COMMAND),
    CONTROL_LANE_COMMAND,
    `job ${jobName} issues the control lane inside a longer line rather than as its own command`,
  );

  // The lane drives records that run under a runner the roadmap job does not
  // have, so the job hosting it has to install one — and a runner being on
  // PATH is only half of what those records need. A compile-contract record
  // builds its fixture project through trybuild, which invokes cargo with
  // `--offline`, so the lock's dependencies have to already be in the
  // registry cache when the lane spawns; the refresh command those records
  // name fetches before it runs anything for exactly that reason. Without it
  // the lane reports the mirror as red BEFORE the mutation — which is the
  // shape of a control that proves nothing — on any runner whose cache is
  // cold, while staying green on a developer machine that fetched months ago.
  // Both prerequisites are therefore resolved against the job body rather
  // than left to a step somebody remembers to keep.
  if (
    byLane(CONTROL_LANE).some((id) => controlCommand(model, control(id)).adapter.runner !== "node")
  ) {
    assert.match(
      body,
      /rust-toolchain@/u,
      `job ${jobName} drives records that run under cargo but installs no toolchain`,
    );
    assert.match(
      body,
      /cargo fetch --locked/u,
      `job ${jobName} drives records whose runner builds offline but never fetches the locked workspace, so the lane's clean run fails on a cold registry before any mutation is written`,
    );
    // `cargo nextest` is a subcommand installed beside the toolchain, not
    // with it: a job that re-applies a nextest-bound record without
    // installing it fails that record's clean run on a missing subcommand,
    // which reads as a refusal of the mirror rather than of the mutation.
    if (
      byLane(CONTROL_LANE).some(
        (id) => controlCommand(model, control(id)).adapter.id === "cargo-nextest",
      )
    )
      assert.match(
        body,
        /install-action@nextest/u,
        `job ${jobName} drives a record whose command is a cargo-nextest run but never installs cargo-nextest`,
      );
    // A lane record that selects `verter_session` runs its default test
    // selection, which embeds the tsgo-backed Svelte projection typecheck
    // gate: under CI those tests hard-fail without the rc typescript
    // launcher, and the launcher ships only through the workspace's
    // `node_modules`, so the job has to install the locked JavaScript
    // toolchain before the lane spawns anything.
    if (
      byLane(CONTROL_LANE).some((id) =>
        selectedPackages(
          controlCommand(model, control(id)).adapter,
          controlCommand(model, control(id)).proof,
        ).includes("verter_session"),
      )
    )
      assert.match(
        body,
        /pnpm install --frozen-lockfile/u,
        `job ${jobName} drives a record whose selection embeds the typecheck gate but never installs the locked JavaScript toolchain its launcher resolves through`,
      );
  }

  // A job that exists and issues the command still decides nothing unless it
  // is ELIGIBLE on the change that would break it. Resolving the trigger
  // filter's patterns below without resolving the gate that consumes them
  // reads as coverage while the job could be conditioned on some unrelated
  // filter — or on nothing, which makes it run on every change to the
  // repository and stop being a signal about these sources at all.
  assert.match(
    body,
    new RegExp(
      String.raw`if:\s*needs\.detect-changes\.outputs\.${CI_TRIGGER_FILTER}\s*==\s*'true'`,
      "u",
    ),
    `job ${jobName} runs the control lane but is not gated on the ${CI_TRIGGER_FILTER} filter whose paths decide what that lane re-applies`,
  );

  // The lane's own deadlines are nested strictly inside the budget the job
  // declares. Inverted, the runner kills the job before the suite reaches its
  // own timeout, so the failure arrives with no diagnostic naming the control
  // that was still running — which is the shape of an incomplete run reported
  // as a result.
  const budget = jobBudgetMs(body);
  assert.ok(budget, `job ${jobName} declares no timeout-minutes`);
  assert.ok(
    CONTROL_LANE_DEADLINE_MS < budget,
    `the control lane's ${CONTROL_LANE_DEADLINE_MS}ms deadline is not strictly inside job ${jobName}'s ${budget}ms budget`,
  );

  // The same nesting for this suite's own job. It stopped being a parse-only
  // suite when it began re-applying controls — it now mirrors the roadmap
  // tree and spawns the validator and the tools it drives as real child
  // processes — so its budget has to be resolved against those deadlines
  // rather than left at whatever sized a cheaper suite.
  const selfLine = `node --test ${selfCommand}`;
  const selfHosting = [...workflowJobs(workflow)].filter(
    ([, jobBody]) => laneCommandLine(jobBody, selfLine) === selfLine,
  );
  assert.equal(
    selfHosting.length,
    1,
    `exactly one job must issue \`${selfLine}\`; ${selfHosting.length} do`,
  );
  const selfBudget = jobBudgetMs(selfHosting[0][1]);
  assert.ok(selfBudget, `job ${selfHosting[0][0]} declares no timeout-minutes`);
  assert.ok(
    INSTRUMENT_LANE_DEADLINE_MS < selfBudget,
    `this lane's ${INSTRUMENT_LANE_DEADLINE_MS}ms re-application deadline is not strictly inside job ${selfHosting[0][0]}'s ${selfBudget}ms budget`,
  );
  assert.ok(
    INSTRUMENT_LANE_COMMAND_DEADLINE_MS < INSTRUMENT_LANE_DEADLINE_MS,
    "a re-applied command may not outlive the case that re-applies it",
  );

  const patterns = triggerPaths(workflow, CI_TRIGGER_FILTER);
  assert.ok(patterns, `the workflow declares no ${CI_TRIGGER_FILTER} filter`);
  // A command-shaped control mutates no file, so it names no subject to
  // cover; the lane's own entry and the module that partitions it are.
  for (const target of [
    CONTROL_LANE_ENTRY,
    "roadmap/0.1.0-tama/tools/closure-controls.mjs",
    ...byLane(CONTROL_LANE)
      .map((id) => control(id).subject)
      .filter(Boolean),
  ])
    assert.ok(
      triggerCovers(patterns, target),
      `${target} decides what the control lane re-applies, but no ${CI_TRIGGER_FILTER} pattern covers it`,
    );
});

// The re-application routine is itself new machinery, and machinery that cannot
// fail is not a control. Two ways it could report a green re-application while
// proving nothing, both planted here against a control this lane already drives:
//
//   - the transcribed refusal stops being the one the command produces, which
//     is exactly how the control this register delegates to the other lane went
//     stale — its transcript described a suite one case smaller than the suite
//     it names, and every count beside it stayed internally consistent;
//   - the mutation is already in the subject, so a "refusal" after writing it
//     is a refusal the tree was producing anyway.
//
// Both are planted in a copy and against a copy of the control row, never in
// the tree or the register under review.
selfAssertion("the re-application routine plants and refuses what the register says", () => {
  // A libtest refusal and its successor one case later differ ONLY in the
  // passing count: the same mutation still fails exactly once. Comparing failed
  // counts alone cannot see that, which is how this register's own lineage
  // control transcribed a binary one case smaller than the binary it names
  // while every number beside it stayed self-consistent.
  const libtest = (passed) =>
    `test result: FAILED. ${passed} passed; 1 failed; 0 ignored; 0 measured; 0 filtered out`;
  assert.notDeepEqual(
    parseRefusal("libtest", libtest(24)),
    parseRefusal("libtest", libtest(25)),
    "a refusal comparison that cannot see a moved passing count greens a stale transcript",
  );
  assert.deepEqual(parseRefusal("libtest", libtest(25)), parseRefusal("libtest", libtest(25)));

  const { model } = analyze(PACKAGE_ROOT);
  const repoRoot = path.resolve(PACKAGE_ROOT, "..", "..");
  const selfCommand = path.relative(repoRoot, fileURLToPath(import.meta.url)).replaceAll("\\", "/");
  const owned = controlsFor(model, INSTRUMENT_LANE, selfCommand);
  const subject = owned.find(
    (row) => controlCommand(model, row).adapter.summary_grammar === "node-test",
  );
  assert.ok(subject, "no control this lane drives transcribes a counted refusal");

  const mirror = mirrorTree(model, repoRoot);
  const env = { ...process.env };
  delete env.NODE_TEST_CONTEXT;
  const spawn = (argv) =>
    spawnSync(process.execPath, argv, {
      cwd: mirror,
      encoding: "utf8",
      env,
      timeout: INSTRUMENT_LANE_COMMAND_DEADLINE_MS,
    });

  // Positive leg first: unaltered, this control re-applies cleanly. Without it
  // the two refusals below could both come from a mirror that was already red.
  reapply({ model, control: subject, mirror, spawn });

  const drifted = {
    ...subject,
    observed: subject.observed.replace(/fail (\d+)/u, (_, n) => `fail ${Number(n) + 1}`),
  };
  assert.notEqual(drifted.observed, subject.observed, "the drift plant did not apply");
  assert.throws(
    () => reapply({ model, control: drifted, mirror, spawn }),
    /the refusal this control transcribes is no longer the one its command produces/u,
    "a transcript whose counts no longer match the live refusal was accepted",
  );

  // The clean run is the record's OWN command, so the record's own counters are
  // re-derived from it rather than transcribed once. That is what closes a
  // record's numbers against a change to the work it selects, so it needs its
  // own refusal: a record whose transcript no longer describes what its command
  // emits must fail before the mutation is even written.
  const boundProof = model.register.proof.find((row) => row.control === subject.id);
  const staleModel = {
    ...model,
    register: {
      ...model.register,
      proof: model.register.proof.map((row) =>
        row.id === boundProof.id
          ? {
              ...row,
              terminal_summary: row.terminal_summary.replace(
                /pass (\d+)/u,
                (_, n) => `pass ${Number(n) + 1}`,
              ),
            }
          : row,
      ),
    },
  };
  assert.notEqual(
    staleModel.register.proof.find((row) => row.id === boundProof.id).terminal_summary,
    boundProof.terminal_summary,
    "the stale-summary plant did not apply",
  );
  assert.throws(
    () => reapply({ model: staleModel, control: subject, mirror, spawn }),
    /transcribes counters its own command no longer produces/u,
    "a record whose transcribed counters no longer match its own clean run was accepted",
  );

  // The bytes the plant actually writes. `String.prototype.replace` reads `$&`,
  // `$1`, `$'`, "$`" and `$$` out of a STRING replacement, so a control mutating
  // any line carrying `${{ ... }}` — every workflow line does — would put bytes
  // in the mirror that are not the ones the register records, and the refusal
  // would then be attributed to a mutation that never landed. Nothing else here
  // can see that: no live control's replacement carries a `$`, so the whole
  // difference between a string and a function replacement is invisible to
  // every other leg, and the routine's own uniqueness and absence checks run
  // BEFORE the write. The subject is therefore read while the command is
  // running, through the caller-supplied spawn, and required to carry the
  // recorded replacement verbatim.
  //
  // The runs are stubbed rather than spawned: this leg is about what the
  // routine WRITES, and a real run of a subject carrying a deliberately
  // `$`-laden edit would fail for its own reasons long before the bytes were
  // compared. The stub returns the record's own transcript for the clean run
  // and the control's own observed refusal for the mutated one, so every other
  // gate in the routine still has to pass on the way through.
  const dollarApplied = `${subject.applied}\n# $& $\` $' $1 $$ \${{ github.sha }}`;
  const dollars = { ...subject, applied: dollarApplied };
  const boundGrammar = controlCommand(model, subject).adapter.summary_grammar;
  assert.equal(boundGrammar, "node-test", "the stubbed transcripts below assume this grammar");
  let mirrored = null;
  let staged = 0;
  const stub = () => {
    staged += 1;
    if (staged === 1) return { status: 0, stdout: boundProof.terminal_summary, stderr: "" };
    mirrored = fs.readFileSync(path.join(mirror, dollars.subject), "utf8");
    return { status: 1, stdout: dollars.observed, stderr: "" };
  };
  reapply({ model, control: dollars, mirror, spawn: stub });
  assert.equal(staged, 2, "the routine did not run the command clean and then mutated");
  assert.ok(
    mirrored.includes(dollarApplied),
    "the plant reached the mirror with its replacement bytes rewritten, so a refusal would be attributed to a mutation the register does not record",
  );
  assert.equal(
    fs.readFileSync(path.join(mirror, dollars.subject), "utf8").includes(dollarApplied),
    false,
    "the routine left its plant behind in the mirror",
  );

  // The plant that was already there. `perl`, `sed` and `grep` all exit 0 on a
  // non-match, so a mutation's exit code is never evidence it landed; the
  // routine has to establish the edit was new before the refusal means anything.
  const planted = path.join(mirror, subject.subject);
  const original = fs.readFileSync(planted, "utf8");
  try {
    fs.writeFileSync(planted, original.replace(subject.reverted, subject.applied));
    assert.throws(
      () => reapply({ model, control: subject, mirror, spawn }),
      /is already present in|does not occur exactly once in/u,
      "a subject already carrying the mutation was re-applied as if the edit were new",
    );
  } finally {
    fs.writeFileSync(planted, original);
  }

  // A subject whose bytes carry CRLF line endings. The validator normalizes
  // before its unique-occurrence checks, so `--check` passes on such a tree;
  // this routine reads the mirror's raw bytes, and a recipe recorded with LF
  // separators occurs zero times in a CRLF subject — the lane would fail a
  // uniqueness the validator already established, on exactly the trees whose
  // bytes differ from the register's spelling. This checkout pins LF, so the
  // leg manufactures the bytes it is about: the stubbed runs return the
  // record's own transcripts, the plant must land anyway, and the original
  // CRLF bytes are what get restored.
  const crlfSubject = path.join(mirror, subject.subject);
  const lfOriginal = fs.readFileSync(crlfSubject, "utf8");
  try {
    fs.writeFileSync(crlfSubject, lfOriginal.replaceAll("\n", "\r\n"));
    let crlfPlanted = null;
    let crlfRuns = 0;
    const crlfSpawn = () => {
      crlfRuns += 1;
      if (crlfRuns === 1) return { status: 0, stdout: boundProof.terminal_summary, stderr: "" };
      crlfPlanted = fs.readFileSync(crlfSubject, "utf8");
      return { status: 1, stdout: subject.observed, stderr: "" };
    };
    reapply({ model, control: subject, mirror, spawn: crlfSpawn });
    assert.ok(
      crlfPlanted?.includes(subject.applied),
      "the plant did not land against a CRLF subject, so a refusal after it would prove nothing",
    );
    assert.equal(
      fs.readFileSync(crlfSubject, "utf8"),
      lfOriginal.replaceAll("\n", "\r\n"),
      "the routine did not restore the CRLF bytes it found",
    );
  } finally {
    fs.writeFileSync(crlfSubject, lfOriginal);
  }

  // A command-shaped control. Its mutation is an argument appended to the
  // recorded command, so the uniqueness checks are over the argument vector:
  // a delta already present is the same defect as a mutation already sitting
  // in the tree. The clean run is the recorded command unchanged and the
  // mutated run is that command plus the delta — in that order, because the
  // clean run is what re-derives the record's counters and a routine that ran
  // only the mutated command would compare its refusal without ever
  // re-deriving them. The runs are stubbed rather than spawned: the legs here
  // are about what the routine RUNS and what it refuses, not about node
  // itself. No file may be touched — a command control mutates no artifact.
  const commandSubject = {
    ...subject,
    kind: "command",
    argv_delta: ["--planted-command-delta"],
    subject: undefined,
    reverted: undefined,
    applied: undefined,
  };
  const commandCalls = [];
  const commandSpawn = (argv) => {
    commandCalls.push(argv);
    return commandCalls.length === 1
      ? { status: 0, stdout: boundProof.terminal_summary, stderr: "" }
      : { status: 1, stdout: subject.observed, stderr: "" };
  };
  const commandProbe = path.join(mirror, subject.subject);
  const commandBefore = fs.readFileSync(commandProbe, "utf8");
  reapply({ model, control: commandSubject, mirror, spawn: commandSpawn });
  assert.equal(
    commandCalls.length,
    2,
    "the routine did not run the command clean and then with the delta",
  );
  assert.deepEqual(
    commandCalls[1],
    [...commandCalls[0], "--planted-command-delta"],
    "the mutated run is not the recorded command plus the recorded delta",
  );
  assert.equal(
    fs.readFileSync(commandProbe, "utf8"),
    commandBefore,
    "a command-shaped control wrote to a file, so it mutated an artifact it does not name",
  );
  assert.throws(
    () =>
      reapply({
        model,
        control: { ...commandSubject, argv_delta: ["--test"] },
        mirror,
        spawn: commandSpawn,
      }),
    /already part of the command/u,
    "a delta naming an argument the command already carries was applied as if it were new",
  );
});

control("a green proof cited for an obligation its run does not reach is refused", () => {
  covers("irrelevant-existing-proof");

  // The case the class is named for: `P-beta` is a real record, it passes, and
  // it is appended to an atom it has nothing to do with. Nothing about the
  // record is broken — that is the point. `A-gamma` requires its evidence to
  // exercise `tools/fixture-gamma.mjs`, and P-beta neither runs nor reads it.
  const irrelevant = run((register) => {
    register.proof[1].covers = ["A-beta", "A-gamma"];
  });
  refuses(irrelevant, "nothing this record runs or reads reaches tools/fixture-gamma.mjs");
  assert.equal(
    irrelevant.model.coverage.get("A-gamma").includes("P-beta"),
    false,
    "an irrelevant record must not warm the atom it was appended to",
  );

  // The positive leg: the record that DOES run the anchored artifact covers it.
  const relevant = run();
  assert.deepEqual(relevant.model.coverage.get("A-gamma"), ["P-gamma"]);

  refuses(
    run((register) => {
      register.proof[1].covers = ["A-beta", "A-not-an-atom"];
    }),
    "covers unknown atom A-not-an-atom",
  );

  // Acyclicity, with nothing volunteered. `P-gamma` runs
  // `tools/fixture-gamma.mjs`; making that file the claim's subject makes the
  // record the subject's own verdict, and the coverage must be refused. The
  // register carries no field a record could omit to escape this.
  const cyclic = run((register) => {
    register.claim.find((claim) => claim.id === "CLM-GAMMA").subject = ["tools/fixture-gamma.mjs"];
  });
  refuses(cyclic, "cyclic coverage");
  assert.equal(
    claimStatus(cyclic.model, "CLM-GAMMA"),
    REFUSED,
    "a cycle must refuse the claim, not sit beside a still-proven one",
  );
  assert.equal(
    cyclic.model.coverage.has("A-gamma"),
    false,
    "cyclic coverage must not warm the atom it could not honestly cover",
  );

  // The SAME rule on the other runner. A cargo command selects work with
  // `-p <package>` and never with a path, so deriving producers from path
  // arguments alone made the rule vacuous for every cargo record — the adapter
  // the Rust successors' claims will use. A selected crate root is the artifact
  // its run is the verdict for, so a claim whose subject lives inside that
  // crate may not be covered by it.
  const cargoCyclic = run((register) => {
    asCargo(register);
    register.proof[2].argv_tail = ["-p", "fixture_crate"];
    register.atom.find((atom) => atom.id === "A-gamma").evidence_anchor =
      "crates/fixture_crate/src/lib.rs";
    register.claim.find((claim) => claim.id === "CLM-GAMMA").subject = [
      "crates/fixture_crate/src/lib.rs",
    ];
    return { crates: ["fixture_crate"] };
  });
  refuses(cargoCyclic, "cyclic coverage — its own run of crates/fixture_crate");
  assert.equal(claimStatus(cargoCyclic.model, "CLM-GAMMA"), REFUSED);

  // The positive leg for the same shape: an unrelated crate subject leaves the
  // record free to cover the atom, so the refusal above is about the cycle and
  // not about cargo records in general.
  const cargoClean = run((register) => {
    asCargo(register);
    register.proof[2].argv_tail = ["-p", "fixture_crate"];
    register.atom.find((atom) => atom.id === "A-gamma").evidence_anchor =
      "crates/fixture_crate/src/lib.rs";
    return { crates: ["fixture_crate"] };
  });
  assert.ok(
    !cargoClean.errors.some((error) => error.includes("cyclic coverage")),
    `an unrelated crate subject is not a cycle; got:\n${cargoClean.errors.join("\n")}`,
  );
  assert.deepEqual(cargoClean.model.coverage.get("A-gamma"), ["P-gamma"]);
});

// A remainder's routing is a shape the authority fixes, not a set membership.
// Every check other than the pinned topology accepts an atom re-routed to a
// different admissible residue and a required owner dropped from a remainder's
// ordered sequence.
control("a transfer or receiving sequence substituted within the allowed set is refused", () => {
  covers("removed-residue-owner");

  refuses(
    run((register) => {
      register.transfer.find((row) => row.atom === "A-hang").residue = ALLOWED_RESIDUES[1];
    }),
    "A-hang=>TCM0-R-TOPOLOGY-SELECTION is not the pinned routing for that atom",
  );
  refuses(
    run((register) => {
      register.transfer.find((row) => row.atom === "A-hang").residue = ALLOWED_RESIDUES[1];
    }),
    `the required transfer A-hang=>${ALLOWED_RESIDUES[0]} is missing`,
  );

  // The owner sequence, not merely the owner set: the fixture's first remainder
  // is received by TCM1 twice, so replacing one row's owner with another
  // authorized same-train descendant is a substitution every other check here
  // accepts.
  refuses(
    run((register) => {
      register.receiving.find(
        (row) => row.residue === ALLOWED_RESIDUES[0] && row.order === 2,
      ).owner_node = "TCM0R";
    }),
    `residue ${ALLOWED_RESIDUES[0]}: receiving owners are TCM1 > TCM0R, not the required TCM1 > TCM1`,
  );

  // A dropped row shortens the sequence rather than emptying it, which the
  // "no receiving criterion" check cannot see.
  refuses(
    run((register) => {
      register.receiving = register.receiving.filter(
        (row) => !(row.residue === ALLOWED_RESIDUES[0] && row.order === 2),
      );
    }),
    `residue ${ALLOWED_RESIDUES[0]}: receiving owners are TCM1, not the required TCM1 > TCM1`,
  );
});

control("a finding filed against a claim must name the atom that discriminates it", () => {
  covers("irrelevant-existing-proof");

  // Routing is an assignment. Without the atom binding a finding could be filed
  // under a claim none of whose obligations speak to it, and the register would
  // still read as complete.
  refuses(
    run((register) => {
      delete register.finding[0].atom;
    }),
    "must name the atom that discriminates it",
  );

  refuses(
    run((register) => {
      register.finding[0].atom = "A-beta";
    }),
    "atom A-beta belongs to CLM-BETA, not CLM-ALPHA",
  );

  refuses(
    run((register) => {
      register.finding[0].atom = "A-invented";
    }),
    "unknown atom A-invented",
  );

  refuses(
    run((register) => {
      register.finding.find((finding) => finding.id === "C2").atom = "A-hang";
    }),
    "a remainder entry is carried by its residue, not an atom",
  );

  // A finding routed at a claim the register does not declare is filed under
  // no summary at all, so nothing counts it as closed or open.
  refuses(
    run((register) => {
      register.finding[0].claim = "CLM-ABSENT";
    }),
    "unknown claim CLM-ABSENT",
  );

  // A must-close entry rerouted to a remainder. The partition is closed in
  // both directions: an entry the ruling requires closed cannot leave through
  // a residue instead.
  refuses(
    run((register) => {
      delete register.finding[0].claim;
      delete register.finding[0].atom;
      register.finding[0].residue = ALLOWED_RESIDUES[0];
    }),
    "must close against a claim, not a residue",
  );
});

control("a selector that matched no work is refused and cannot cover anything", () => {
  covers("zero-selected-work");

  const { errors, model } = run((register) => {
    Object.assign(register.proof[2], {
      selected: 0,
      executed: 0,
      passed: 0,
      terminal_summary: "fixture-gamma: PASS cases=0",
    });
  });
  refuses({ errors }, "proof P-gamma: zero selected work");
  assert.equal(claimStatus(model, "CLM-GAMMA"), REFUSED);
  assert.equal(model.coverage.has("A-gamma"), false, "a refused proof must not warm coverage");
});

control(
  "skips must equal the count the record declared, and a declared count must say what it covers",
  () => {
    covers("skipped-work");

    refuses(
      run((register) => {
        Object.assign(register.proof[1], { selected: 5, executed: 2, skipped: 3 });
      }),
      "3 skips against 0 declared",
    );

    refuses(
      run((register) => {
        Object.assign(register.proof[1], {
          selected: 5,
          executed: 2,
          skipped: 3,
          expected_skips: 3,
        });
      }),
      "a declared skip count must state its basis",
    );

    refuses(
      run((register) => {
        Object.assign(register.proof[1], {
          selected: 5,
          executed: 2,
          skipped: 2,
          expected_skips: 3,
          skip_basis: "drifted",
        });
      }),
      "2 skips against 3 declared",
    );

    refuses(
      run((register) => {
        register.proof[1].skip_basis = "declared without a count";
      }),
      "skip basis stated without a declared skip count",
    );

    // The positive leg: a declared, described skip count is admissible, and the
    // count stays visible in the generated view rather than being absorbed.
    const declared = run((register) => {
      Object.assign(register.proof[1], {
        selected: 5,
        executed: 2,
        passed: 2,
        skipped: 3,
        expected_skips: 3,
        skip_basis: "three cases the suite marks ignored",
        adapter: "node-runner",
        terminal_summary: "tests 5 | pass 2 | fail 0 | cancelled 0 | skipped 3 | todo 0",
      });
      delete register.proof[1].count_key;
      register.adapter.push({
        id: "node-runner",
        runner: "node",
        argv_prefix: ["--test"],
        summary_shape: "node:test terminal block",
        summary_grammar: "node-test",
        reexecution: "instrument",
      });
      return withSkipBasis("P-beta", "three cases the suite marks ignored");
    });
    assert.deepEqual(declared.errors, []);
    assert.equal(claimStatus(declared.model, "CLM-BETA"), PROVEN_BOUNDED);
    assert.match(
      renderView(declared.model),
      /declared skips: three cases the suite marks ignored/u,
    );
  },
);

control("counters that do not reconcile are refused", () => {
  covers("inconsistent-counters");

  refuses(
    run((register) => {
      register.proof[0].passed = 3;
    }),
    "executed != passed + failed",
  );

  refuses(
    run((register) => {
      register.proof[0].selected = 6;
    }),
    "selected != executed + skipped",
  );

  const failing = run((register) => {
    Object.assign(register.proof[0], { passed: 3, failed: 1 });
  });
  refuses(failing, "proof P-alpha: 1 failed");
  assert.equal(claimStatus(failing.model, "CLM-ALPHA"), REFUSED);
});

control("a control whose mutation is unproven or unused is refused", () => {
  covers("unapplied-mutation");

  refuses(
    run((register) => {
      register.control[0].observed = "PLACEHOLDER";
    }),
    "observed outcome is still a placeholder",
  );

  refuses(
    run((register) => {
      register.control.push({
        id: "CTL-unused",
        kind: "command",
        mutation: "Never exercised.",
        argv_delta: ["--never"],
        uniqueness: "unique-new-occurrence",
        observed: "nothing",
      });
    }),
    "control CTL-unused: referenced by 0 proofs",
  );

  refuses(
    run((register) => {
      register.proof[1].control = "CTL-alpha";
    }),
    "control CTL-alpha: referenced by 2 proofs",
  );

  refuses(
    run((register) => {
      register.control[0].uniqueness = "probably-unique";
    }),
    "expected one of",
  );
});

// `unique-new-occurrence` used to be a literal the author typed. Both halves are
// now resolved against the tree the register ships with: the replaced text must
// be there exactly once, and the introduced text must not be there at all.
control("a control mutation that could not have applied, or is still applied, is refused", () => {
  covers("unapplied-mutation");

  // The mutation names text that is not in the subject: it could never have
  // been applied, so nothing was discriminated.
  refuses(
    run((register) => {
      register.control[0].reverted = "export const absent = 1;\nexport const missing = 2;";
    }),
    "occurs 0 times",
  );

  // The mutation names text that appears twice: applying it was not the unique
  // application the record claims.
  refuses(
    run((register) => {
      register.control[0].reverted = "export const repeated = 0;\nexport const tail = 9;";
    }),
    "occurs 2 times",
  );

  // The mutation is still in the tree, so the tree under review is the mutated
  // one and the record's own subject disproves the revert.
  refuses(
    run((register) => {
      register.control[0].applied = "export const alpha = 1;\nexport const beta = 2;";
    }),
    "is still present in",
  );

  // A single-line half of a file this register owns would match the record's
  // own text, so the control would be proving something about itself.
  refuses(
    run((register) => {
      register.control[0].reverted = "export const alpha = 1;";
    }),
    "must span a line boundary",
  );

  refuses(
    run((register) => {
      delete register.control[0].applied;
    }),
    "a source mutation must name applied",
  );

  refuses(
    run((register) => {
      register.control[0].subject = "tools/never-written.mjs";
    }),
    "subject tools/never-written.mjs does not resolve",
  );

  // A command mutation is new only if its arguments are not already in the
  // command it claims to have changed.
  refuses(
    run((register) => {
      register.control[2].argv_delta = ["tools/fixture-gamma.mjs"];
    }),
    "is already part of the command it claims to have mutated",
  );

  // A control's outcome is a REFUSAL. Transcribing a clean run as the observed
  // outcome records nothing.
  refuses(
    run((register) => {
      register.control[0].observed = "fixture-alpha: PASS cases=4";
    }),
    "records no refusal",
  );

  // Neither does DESCRIBING one. A sentence about a failure is exactly what a
  // mutation that was never planted, or planted and never run, leaves behind:
  // the recorded outcome has to be the runner's own refusal, in the runner's
  // own grammar, which is not something an author can write from memory.
  refuses(
    run((register) => {
      register.control[0].observed = "it broke something and the run went red, exit 1";
    }),
    "carries no tool-line refusal this check can read",
  );
  refuses(
    run((register) => {
      asCargo(register);
      register.control[0].observed = "the identity case stopped failing to compile";
    }),
    "carries no libtest refusal this check can read",
  );

  // Each grammar's own refusal, and each grammar's own CLEAN run, so a reader
  // that accepted anything is the hole this closes.
  for (const [grammar, refusal, clean] of [
    [
      "libtest",
      "test result: FAILED. 3 passed; 1 failed; 0 ignored; 0 measured; 0 filtered out",
      "test result: ok. 4 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out",
    ],
    [
      "nextest",
      "Summary [ 1.000s] 4 tests run: 3 passed, 1 failed, 0 skipped",
      "Summary [ 1.000s] 4 tests run: 4 passed, 0 skipped",
    ],
    [
      "node-test",
      "tests 4 | pass 3 | fail 1 | cancelled 0 | skipped 0 | todo 0",
      "tests 4 | pass 4 | fail 0 | cancelled 0 | skipped 0 | todo 0",
    ],
    ["tool-line", "ERROR: the fixture refused the mutated register", "fixture-alpha: PASS cases=4"],
    [
      "compile-contracts",
      "compile contracts: owner=identity, fixtures=2\ntest tests/compile-fail/one.rs ... ok",
      "compile contracts: owner=identity, fixtures=2\ntest tests/compile-fail/one.rs ... ok\ntest tests/compile-fail/two.rs ... ok",
    ],
  ]) {
    assert.ok(parseRefusal(grammar, refusal, "cases"), `${grammar} did not read its own refusal`);
    assert.equal(
      parseRefusal(grammar, clean, "cases"),
      null,
      `${grammar} read a clean run as a refusal`,
    );
    assert.equal(
      parseRefusal(grammar, "the run went red", "cases"),
      null,
      `${grammar} read prose as a refusal`,
    );
  }
  // A selector that matched nothing is a refusal even though no case failed —
  // it is the shape a command mutation produces, and reading only the failed
  // count would miss it.
  assert.ok(
    parseRefusal("nextest", "Summary [ 0.018s] 0 tests run: 0 passed, 9438 skipped"),
    "a run that selected nothing is a refusal",
  );

  // A source control may not carry command fields, and vice versa: the two
  // shapes are checked differently and a mixed row would escape both.
  refuses(
    run((register) => {
      register.control[0].argv_delta = ["--extra"];
    }),
    "argv_delta belongs to a command mutation",
  );

  refuses(
    run((register) => {
      register.control[2].subject = FIXTURE_SUBJECT;
    }),
    "subject belongs to a source mutation",
  );

  // Both halves identical: the edit changed nothing, so the refusal the record
  // transcribes is whatever the tree was already producing.
  refuses(
    run((register) => {
      register.control[0].applied = register.control[0].reverted;
    }),
    "the mutation replaces its text with itself",
  );

  // A command-shaped mutation carries its newness in the arguments it added.
  // Without them there is nothing to check against the recorded command, and
  // the control describes an edit that cannot be located at all.
  refuses(
    run((register) => {
      delete register.control[2].argv_delta;
    }),
    "a command mutation must name the arguments it added",
  );
});

control("a derived limit forces bounded status and is not admissible on its own", () => {
  covers("disclosed-limit");

  // `P-beta` becomes a record this instrument cannot re-run, bound to a lane
  // whose filter is the crate tree — while the artifacts it cites live under
  // `contracts/`. Nothing in that lane can ever notice a change to its inputs,
  // and the limit that follows is DERIVED from the workflow rather than
  // disclosed by the author.
  const { errors, model } = run((register) => {
    asCargo(register);
  });
  limits({ model }, "P-beta", "outside the rust trigger paths");
  assert.equal(claimStatus(model, "CLM-BETA"), PROVEN_BOUNDED);
  assert.equal(model.claimStatus.get("CLM-BETA").admissible, false);
  refuses({ errors }, "claim CLM-BETA: bounded without an approved transfer");
  // The limit is reported WITH the record it came from, not only as a status.
  // A bounded claim whose diagnostic names no record leaves the reader to
  // rediscover which of its proofs stopped reaching, which is how a limit
  // becomes a label instead of a finding.
  assert.ok(
    errors.some((error) => error.startsWith("claim CLM-BETA: bounded by P-beta — ")),
    errors.join("\n"),
  );
  assert.match(renderView(model), /outside the rust trigger paths/u);
});

control("a bounded claim cannot become proven by dropping or misdirecting its transfer", () => {
  covers("bounded-to-proven-laundering");

  const dropped = run((register) => {
    register.transfer = register.transfer.filter((row) => row.atom !== "A-hang");
  });
  assert.equal(
    claimStatus(dropped.model, "CLM-BETA"),
    OPEN,
    "deleting a transfer must open the claim, not prove it",
  );
  assert.deepEqual(dropped.model.claimStatus.get("CLM-BETA").uncovered, ["A-hang"]);
  assert.equal(certification(dropped).ok, false, "an OPEN claim must not be certifiable either");

  refuses(
    run((register) => {
      register.transfer[0].residue = "TCM0-R-INVENTED";
    }),
    "unknown residue TCM0-R-INVENTED",
  );

  refuses(
    run((register) => {
      register.transfer[0].approved_by = "decisions/unapproved.md";
    }),
    "transfer A-hang",
  );

  refuses(
    run((register) => {
      register.atom.find((atom) => atom.id === "A-hang").received_by = "TCM1-AC1";
    }),
    "transferred atoms are received through their residue",
  );
});

// A record's counters used to sit BESIDE its summary, so any plausible
// sentence with self-consistent numbers was evidence. They are now read out of
// the transcript itself under the shape the adapter's runner emits, which is
// what makes a drifted count, a prose paraphrase, and a transcribed failure
// distinguishable from a real terminal line.
control("counters are read out of the transcript, not accepted beside it", () => {
  covers("stale-evidence");

  // The count drifted after the run: the summary still says four, the record
  // now claims five, and the numbers reconcile with each other perfectly.
  refuses(
    run((register) => {
      Object.assign(register.proof[0], { selected: 5, executed: 5, passed: 5 });
    }),
    "selected is 5, but its transcript states 4",
  );

  // A summary nobody's terminal ever printed.
  refuses(
    run((register) => {
      register.proof[0].terminal_summary = "the alpha suite was green when I ran it";
    }),
    "terminal summary is not a tool-line summary",
  );

  // A transcribed FAILING run, with the counters that would make it look clean.
  refuses(
    run((register) => {
      register.proof[0].terminal_summary = "fixture-alpha: FAIL cases=4";
    }),
    "terminal summary is not a tool-line summary",
  );

  // The count key must name a field the tool's own line carries.
  refuses(
    run((register) => {
      register.proof[0].count_key = "invented";
    }),
    "terminal summary is not a tool-line summary",
  );

  refuses(
    run((register) => {
      delete register.proof[0].count_key;
    }),
    "must name the key that carries its count",
  );

  // Every grammar is exercised against a summary of a DIFFERENT runner, because
  // a grammar that accepts anything is the hole this replaces.
  const foreign = {
    libtest: "fixture-alpha: PASS cases=4",
    nextest: "test result: ok. 4 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out",
    "node-test": "Summary [ 1.000s] 4 tests run: 4 passed, 0 skipped",
    "tool-line": "tests 4 | pass 4 | fail 0 | cancelled 0 | skipped 0 | todo 0",
    "compile-contracts": "tests 4 | pass 4 | fail 0 | cancelled 0 | skipped 0 | todo 0",
  };
  assert.deepEqual(Object.keys(foreign).sort(), [...SUMMARY_GRAMMARS].sort());
  for (const [grammar, summary] of Object.entries(foreign))
    assert.equal(
      parseTerminalSummary(grammar, summary, "cases"),
      null,
      `${grammar} accepted a summary of another runner`,
    );

  // libtest states its verdict in a word AND in counts, and the two are not
  // redundant: a harness that aborts, or a binary that could not start, prints
  // `FAILED` with `0 failed`. Reading the counts while discarding the word
  // admitted exactly that transcript as a clean run.
  assert.equal(
    parseTerminalSummary(
      "libtest",
      "test result: FAILED. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out",
    ),
    null,
    "a libtest run that reported FAILED is not evidence, whatever its counts say",
  );
  // One failing binary among green siblings is still a failing transcription.
  assert.equal(
    parseTerminalSummary(
      "libtest",
      "test result: ok. 8 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out | test result: FAILED. 2 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out",
    ),
    null,
    "a FAILED binary anywhere in the transcription refuses the record",
  );
  refuses(
    run((register) => {
      asCargo(register);
      register.proof[0].terminal_summary =
        "test result: FAILED. 4 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out";
    }),
    "terminal summary is not a libtest summary",
  );

  // The positive leg: each grammar reads the counts its own runner prints.
  assert.deepEqual(
    parseTerminalSummary(
      "libtest",
      "test result: ok. 26 passed; 0 failed; 3 ignored; 0 measured; 0 filtered out | test result: ok. 8 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out",
    ),
    { selected: 37, executed: 34, passed: 34, failed: 0, skipped: 3 },
  );
  assert.deepEqual(
    parseTerminalSummary("nextest", "Summary [ 183.180s] 8889 tests run: 8889 passed, 549 skipped"),
    { selected: 9438, executed: 8889, passed: 8889, failed: 0, skipped: 549 },
  );
  // A cancelled case did not pass and a todo case did not run, so neither may
  // be absorbed into a clean count.
  assert.deepEqual(
    parseTerminalSummary(
      "node-test",
      "tests 9 | pass 6 | fail 1 | cancelled 1 | skipped 1 | todo 0",
    ),
    { selected: 9, executed: 8, passed: 6, failed: 2, skipped: 1 },
  );
  // The same block as the runner actually prints it, one count per line behind
  // the reporter's marker. A count must be read from the line it opens, not
  // from wherever the digits happen to sit.
  assert.deepEqual(
    parseTerminalSummary(
      "node-test",
      "✔ a case whose name mentions tests 999 (1ms)\nℹ tests 9\nℹ suites 0\nℹ pass 6\nℹ fail 1\nℹ cancelled 1\nℹ skipped 1\nℹ todo 0\n",
    ),
    { selected: 9, executed: 8, passed: 6, failed: 2, skipped: 1 },
  );

  // The compile-fail runner announces its fixture count BEFORE running, so the
  // banner alone is a selection, not a result. A run that printed it and then
  // failed must not read as complete.
  const okLine = (name) => `test tests/compile-fail/${name}.rs ... ok`;
  assert.deepEqual(
    parseTerminalSummary("compile-contracts", "compile contracts: owner=identity, fixtures=2"),
    { selected: 2, executed: 2, passed: 0, failed: 2, skipped: 0 },
  );
  assert.deepEqual(
    parseTerminalSummary(
      "compile-contracts",
      `compile contracts: owner=identity, fixtures=2\n${okLine("one")}\n${okLine("two")}`,
    ),
    { selected: 2, executed: 2, passed: 2, failed: 0, skipped: 0 },
  );

  // An adapter the allowlist does not carry has no declared runner, argv
  // prefix or grammar, so nothing about the record it serves is derivable.
  refuses(
    run((register) => {
      register.proof[0].adapter = "shell-tool";
    }),
    "adapter shell-tool is not allowlisted",
  );

  // A record whose summary was never filled in transcribes no run.
  refuses(
    run((register) => {
      register.proof[1].terminal_summary = "PLACEHOLDER";
    }),
    "terminal summary is still a placeholder",
  );

  // A count key names the field a tool-line carries its count in. On any other
  // grammar the counts come out of the block itself, so a key there is a
  // declaration nothing reads.
  refuses(
    run((register) => {
      register.adapter[0].summary_grammar = "node-test";
    }),
    "a count key is only meaningful for a tool-line summary",
  );

  // A runner that computes its own selection from a directory has its count
  // RE-DERIVED from that directory rather than trusted, so the record has to
  // cite one directory, that directory has to resolve, and the count it holds
  // has to be the count the record transcribes.
  refuses(
    run((register) => {
      register.adapter[0].count_source = "fixture-directory";
      register.proof[0].fixtures = ["crates/fixture-owned/src/lib.rs", "contracts/fixture.md"];
    }),
    "a directory-counted record must cite fixtures from exactly one directory",
  );
  refuses(
    run((register) => {
      register.adapter[0].count_source = "fixture-directory";
      register.proof[0].fixtures = ["crates/fixture-owned/src/lib.rs"];
    }),
    "cases the runner would select",
  );
  refuses(
    run((register) => {
      register.adapter[0].count_source = "fixture-directory";
      register.proof[0].fixtures = ["crates/absent-fixtures/one.rs"];
    }),
    "fixture directory crates/absent-fixtures does not resolve",
  );
});

// Re-execution is what keeps a transcription current, so which records get it
// is a capability of the adapter rather than a property a record volunteers.
control("a runner this instrument can invoke may not opt out of re-execution", () => {
  covers("stale-evidence");

  refuses(
    run((register) => {
      register.adapter[0].reexecution = "external";
    }),
    "re-executed rather than trusted",
  );

  refuses(
    run((register) => {
      register.adapter[0].refresh_job = FIXTURE_REFRESH_JOB;
    }),
    "meaningless on a record the control suite re-runs itself",
  );

  refuses(
    run((register) => {
      asCargo(register, { refresh_job: undefined });
      delete register.adapter[0].refresh_job;
    }),
    "must declare refresh_job",
  );
});

// The record's selector must name work that exists. A package this workspace
// does not have could not have produced the summary beside it.
control("a command selecting a package the workspace does not have is refused", () => {
  covers("irrelevant-existing-proof");

  refuses(
    run((register) => {
      asCargo(register);
      register.proof[0].argv_tail = ["-p", "verter_not_a_crate"];
    }),
    "package verter_not_a_crate is not a crate of this workspace",
  );

  refuses(
    run((register) => {
      asCargo(register);
      register.proof[0].argv_tail = ["-p"];
    }),
    "-p names no package",
  );
});

// A transcription stays true only while nothing it observed changes, so the
// lane that would notice must be resolved rather than assumed. For a record
// this instrument re-runs that lane is its own; for one it cannot, the record
// names the lane that does.
control("evidence outside the trigger paths of the lane that re-runs it is refused", () => {
  covers("missing-dependency");

  refuses(
    run(() => ({
      workflow: FIXTURE_WORKFLOW.replace("              - 'contracts/**'\n", ""),
    })),
    "no tama trigger path covers it",
  );

  refuses(
    run(() => ({
      workflow: FIXTURE_WORKFLOW.replace(
        "    if: needs.detect-changes.outputs.tama == 'true'\n",
        "",
      ),
    })),
    "is not gated on the tama filter",
  );

  refuses(
    run(() => ({
      workflow: FIXTURE_WORKFLOW.replace(INSTRUMENT_COMMANDS[1], "node --test something-else.mjs"),
    })),
    "expected exactly one job running the instrument, found 0",
  );

  refuses(
    run(() => ({
      workflow: FIXTURE_WORKFLOW.replace("            tama:\n", "            other:\n"),
    })),
    "no tama trigger filter",
  );

  // A pattern this check cannot resolve must fail rather than be assumed to
  // cover whatever it was meant to. A trailing subtree is not on its own
  // enough: a star in the PREFIX names one subtree per matching directory,
  // which this check cannot expand.
  for (const unreadable of ["contracts/**/*.md", "contracts/*/**"])
    refuses(
      run(() => ({
        workflow: FIXTURE_WORKFLOW.replace("- 'contracts/**'", `- '${unreadable}'`),
      })),
      "is not a form this check resolves",
    );

  // A quoting a YAML sequence admits is not a shape this reader may stop at.
  // Truncating the pattern list there reports a filter as narrower than it is,
  // which turns a covered artifact into a hard error about a path the lane
  // plainly names.
  for (const requoted of ['- "contracts/**"', "- contracts/**", "- contracts/** # the tree"]) {
    const { errors } = run(() => ({
      workflow: FIXTURE_WORKFLOW.replace("- 'contracts/**'", requoted),
    }));
    assert.deepEqual(
      errors.filter((error) => error.includes("no tama trigger path covers it")),
      [],
      `${requoted} declares the same pattern the quoted form does`,
    );
  }

  // The conservative direction has a cost: reporting a readable shape as
  // unreadable turns a covered artifact into a hard error. A single star inside
  // a final segment is a shape the live lanes use, so it resolves and covers.
  {
    const { errors } = run(() => ({
      workflow: FIXTURE_WORKFLOW.replace("- 'contracts/**'", "- 'contracts/*.md'"),
    }));
    assert.deepEqual(
      errors.filter((error) => error.includes("is not a form this check resolves")),
      [],
      "a single star inside a final path segment is a shape this check reads",
    );
    assert.deepEqual(
      errors.filter((error) => error.includes("no tama trigger path covers it")),
      [],
      "and a pattern it reads must credit the artifact it really matches",
    );
  }
});

// An external record's lane is a declaration about the workflow, so it is
// resolved against the workflow: the job must exist, must issue the command,
// and must be gated on the filter its inputs are measured against.
control("an external record's declared refresh lane is resolved, not believed", () => {
  covers("missing-dependency");

  refuses(
    run((register) => {
      asCargo(register, { refresh_job: "a-job-that-does-not-exist" });
    }),
    "a-job-that-does-not-exist is not a job of the workflow",
  );

  // No workflow at all. Every lane resolution and every trigger-path
  // measurement reads it, so its absence is an unresolved input rather than an
  // empty one — silence there would credit every lane declaration at once.
  refuses(
    run(() => ({ workflow: null })),
    "ci workflow: ENOENT",
  );

  refuses(
    run((register) => {
      asCargo(register, { refresh_command: "cargo run --something-else" });
    }),
    'does not run "cargo run --something-else"',
  );

  // Containment accepts a PREFIX, and a prefix that stops before the lane's own
  // selection arguments hides exactly the flags that decide what it runs. The
  // declaration must be the line the job issues, whole.
  refuses(
    run((register) => {
      asCargo(register, { refresh_command: "cargo nextest run" });
      return {
        workflow: FIXTURE_WORKFLOW.replace(
          FIXTURE_REFRESH_COMMAND,
          'cargo nextest run -E "package(unrelated)"',
        ),
      };
    }),
    "is only a prefix of the lane's command",
  );

  // A refreshing lane's reach is measured against its own filter's patterns, so
  // a shape this check cannot read is reported there as it is in the
  // instrument's own filter rather than passing as a silent non-match.
  refuses(
    run((register) => {
      asCargo(register);
      return {
        workflow: FIXTURE_WORKFLOW.replace(
          "            rust:\n              - 'crates/**'",
          "            rust:\n              - 'crates/*/**'",
        ),
      };
    }),
    "rust trigger pattern crates/*/** is not a form this check resolves",
  );

  refuses(
    run((register) => {
      asCargo(register);
      return {
        workflow: FIXTURE_WORKFLOW.replace(
          "    if: needs.detect-changes.outputs.rust == 'true'\n",
          "",
        ),
      };
    }),
    `job ${FIXTURE_REFRESH_JOB} is not gated on the rust filter`,
  );

  refuses(
    run((register) => {
      asCargo(register, { refresh_filter: "a-filter-nobody-declared" });
    }),
    "declares no a-filter-nobody-declared trigger filter",
  );

  // Naming a job that issues the command is not yet a statement that the job
  // runs THIS record's work: a lane selecting a narrower universe leaves the
  // record's packages unrefreshed while the job, the command, and the gate all
  // resolve. An unresolvable selection is a derived limit, and a limited claim
  // is bounded without an approved transfer.
  const limitsMatching = ({ model }, fragment) =>
    [...model.proofLimits.values()].flat().filter((limit) => limit.includes(fragment));

  const baseline = run((register) => {
    asCargo(register);
  });
  assert.deepEqual(
    limitsMatching(baseline, "selection"),
    [],
    "a lane that selects the whole workspace resolves",
  );

  const narrow = run((register) => {
    asCargo(register, { refresh_command: "cargo nextest run --fixture" });
    return {
      workflow: FIXTURE_WORKFLOW.replace(FIXTURE_REFRESH_COMMAND, "cargo nextest run --fixture"),
    };
  });
  assert.ok(
    limitsMatching(narrow, "names no selection this check can resolve").length > 0,
    `expected a derived selection limit; got ${JSON.stringify([...narrow.model.proofLimits])}`,
  );
  assert.equal(
    narrow.model.claimStatus.get("CLM-ALPHA").admissible,
    false,
    "an unresolved refreshing selection is a limit, and a limited claim is inadmissible",
  );

  // An archive-consuming lane resolves only when some job in the same workflow
  // builds that exact archive over the whole workspace.
  const archiveCommand = "cargo nextest run --archive-file artifacts/fixture.tar.zst";
  const archived = (builder, { needsBuilder = true } = {}) =>
    run((register) => {
      asCargo(register, { refresh_command: archiveCommand });
      const workflow = FIXTURE_WORKFLOW.replace(
        `      - run: ${FIXTURE_REFRESH_COMMAND}`,
        `      - run: ${archiveCommand}\n\n  archive-build:\n    runs-on: ubuntu-latest\n    steps:\n      - run: ${builder}`,
      );
      return {
        workflow: needsBuilder
          ? workflow.replace(
              `  ${FIXTURE_REFRESH_JOB}:\n    needs: detect-changes`,
              `  ${FIXTURE_REFRESH_JOB}:\n    needs: [detect-changes, archive-build]`,
            )
          : workflow,
      };
    });
  const resolved = archived(
    "cargo nextest archive --workspace --archive-file artifacts/fixture.tar.zst",
  );
  assert.deepEqual(limitsMatching(resolved, "artifacts/fixture.tar.zst"), []);
  assert.ok(
    limitsMatching(
      archived("cargo nextest archive -p one-package --archive-file artifacts/fixture.tar.zst"),
      "no job in this workflow builds artifacts/fixture.tar.zst over the whole workspace",
    ).length > 0,
    "a lane whose archive is not built over the workspace does not resolve",
  );
  // A job somewhere in the workflow building the archive is not the same as
  // THIS lane consuming it: without the dependency edge the two are unrelated.
  assert.ok(
    limitsMatching(
      archived("cargo nextest archive --workspace --archive-file artifacts/fixture.tar.zst", {
        needsBuilder: false,
      }),
      "does not declare \`archive-build\` among its needs",
    ).length > 0,
    "a lane that does not depend on the archive builder does not resolve",
  );
  // The builder is the job with ONE command that both selects the workspace and
  // names the archive, not a job that happens to contain the two separately.
  assert.ok(
    limitsMatching(
      archived(
        "cargo nextest archive -p one-package --archive-file artifacts/fixture.tar.zst\n      - run: cargo build --workspace",
      ),
      "no job in this workflow builds artifacts/fixture.tar.zst over the whole workspace",
    ).length > 0,
    "two unrelated steps in one job do not build a whole-workspace archive",
  );
  // A run-time narrowing the lane applies is PUBLISHED, not folded away: the
  // lane executing work drawn from a record's packages is not the lane
  // executing that record's selection.
  const partitioned = run((register) => {
    const command = `${archiveCommand} --partition hash:1/4`;
    asCargo(register, { refresh_command: command });
    return {
      workflow: FIXTURE_WORKFLOW.replace(
        `      - run: ${FIXTURE_REFRESH_COMMAND}`,
        `      - run: ${command}\n\n  archive-build:\n    runs-on: ubuntu-latest\n    steps:\n      - run: cargo nextest archive --workspace --archive-file artifacts/fixture.tar.zst`,
      ).replace(
        `  ${FIXTURE_REFRESH_JOB}:\n    needs: detect-changes`,
        `  ${FIXTURE_REFRESH_JOB}:\n    needs: [detect-changes, archive-build]`,
      ),
    };
  });
  assert.deepEqual(limitsMatching(partitioned, "artifacts/fixture.tar.zst"), []);
  assert.match(
    partitioned.model.proofRefresh.get("P-alpha"),
    /narrowed at run time by `--partition hash:1\/4`/u,
    "a lane that narrows its own universe must publish the narrowing",
  );

  // A record's own re-derived selected count is a property of the RECORD. It
  // says nothing about what the lane selects, so it may not resolve the lane.
  const counted = run((register) => {
    asCargo(register, {
      refresh_command: "cargo nextest run --fixture",
      count_source: "fixture-directory",
    });
    return {
      workflow: FIXTURE_WORKFLOW.replace(FIXTURE_REFRESH_COMMAND, "cargo nextest run --fixture"),
    };
  });
  assert.ok(
    limitsMatching(counted, "names no selection this check can resolve").length > 0,
    "a re-derived selected count does not resolve the lane's own selection",
  );

  // The reach is PUBLISHED, not left to prose: a reader of the generated view
  // must be able to tell a record whose counts this instrument re-derives from
  // one whose lane merely re-runs the work.
  assert.match(resolved.model.proofRefresh.get("P-alpha"), /archive-build/u);
  assert.match(resolved.model.proofRefresh.get("P-alpha"), /does not re-derive these counts/u);
  assert.match(
    run().model.proofRefresh.get("P-alpha"),
    /compares the counts its own run derives/u,
    "a re-executed record must not publish the weaker guarantee",
  );

  // The two halves of an ENUMERATED lane universe are declared together or
  // not at all. One without the other resolves nothing: an enumeration with no
  // flag never meets the record's own selection, and a flag with nothing to
  // enumerate is a name published where a resolution was owed.
  refuses(
    run((register) => {
      asCargo(register, { refresh_selection_enumerator: "node tools/fixture-owners.mjs" });
    }),
    "an enumerated lane universe must name the flag that carries a record",
  );
  refuses(
    run((register) => {
      asCargo(register, { refresh_selection_flag: "--features" });
    }),
    "refresh_selection_flag resolves nothing without the enumeration it is matched against",
  );

  // The mirror case for the run-time predicate: a producer declared for a lane
  // whose command narrows by nothing describes a narrowing that is not there.
  refuses(
    run((register) => {
      asCargo(register, { refresh_selection_producer: "node tools/fixture-filter.mjs" });
    }),
    "names a producer for a narrowing this lane does not carry",
  );
});

// The instrument's own lane must be triggered by the artifacts the VALIDATOR
// opens, which are not the same set as the artifacts a record's TESTS exercise.
// Attributing a validator input only to the lane that re-runs the tests let an
// ordinary change to that input turn `--check` red on a pull request where this
// lane is not even eligible.
control("an artifact this validator reads must trigger this validator's own lane", () => {
  covers("missing-dependency");

  // A cargo record's fixtures, control subject, package manifest and evidence
  // anchor are all read by `analyze` itself. Dropping the crate tree from the
  // instrument's own filter must fail even though the record's refreshing lane
  // still covers it.
  const dropped = run((register) => {
    asCargo(register);
    register.proof[2].argv_tail = ["-p", "fixture_crate"];
    register.atom.find((atom) => atom.id === "A-gamma").evidence_anchor =
      "crates/fixture_crate/src/lib.rs";
    return {
      crates: ["fixture_crate"],
      workflow: FIXTURE_WORKFLOW.replace(
        "              - 'tools/**'\n              - 'crates/**'\n",
        "              - 'tools/**'\n",
      ),
    };
  });
  refuses(dropped, "crates/fixture_crate/Cargo.toml is cited as evidence but no tama trigger path");
  refuses(dropped, "crates/fixture_crate/src/lib.rs is cited as evidence but no tama trigger path");

  // A filter may EXCLUDE. Reading `!pattern` as an ordinary pattern would
  // credit the lane with coverage it explicitly does not have, which is the one
  // unsound direction available to this check.
  refuses(
    run(() => ({
      workflow: FIXTURE_WORKFLOW.replace(
        "              - 'tools/**'\n",
        "              - 'tools/**'\n              - '!tools/fixture-gamma.mjs'\n",
      ),
    })),
    "tools/fixture-gamma.mjs is cited as evidence but no tama trigger path covers it",
  );
});

// --- the suite's own completeness ------------------------------------------

// Pinning identifiers closes half the hole. A claim, an atom, or a finding can
// keep its id, its coverage and its derived status while what it ASSERTS is
// rewritten to something the existing evidence already shows — a proposition
// weakened to fit its proof, with an unchanged count and a status that never
// leaves PROVEN. Nothing in the set checks can see that, because nothing about
// the set changed.
control("a proposition rewritten under a pinned id is refused", () => {
  covers("hollowed-statement");

  refuses(
    run((register) => {
      register.atom[0].statement = "Something weaker that the same evidence already shows.";
    }),
    "statement pin: atom:A-stated asserts",
  );
  refuses(
    run((register) => {
      register.claim[0].statement = "Alpha claim, quietly narrowed to what is already true.";
    }),
    "statement pin: claim:CLM-ALPHA asserts",
  );
  refuses(
    run((register) => {
      register.finding[0].statement = "Restated as something that was never a defect.";
    }),
    "statement pin: finding:C1 asserts",
  );

  // The status is the whole point: the weakened claim still derives PROVEN, so a
  // reader watching the derivation alone sees nothing at all.
  const hollowed = run((register) => {
    register.atom[0].statement = "Weaker.";
  });
  assert.equal(
    claimStatus(hollowed.model, "CLM-ALPHA"),
    "PROVEN",
    "the weakened proposition still derives PROVEN, so the pin is what discriminates it",
  );

  // Reflowing a paragraph is not rewriting it. A pin that fired on layout would
  // be repinned on every wrap and stop being read.
  const reflowed = run((register) => {
    register.atom[0].statement = "A stated contract   obligation.";
  });
  assert.deepEqual(
    reflowed.errors.filter((error) => error.includes("statement pin")),
    [],
    "whitespace is normalised before the proposition is digested",
  );

  // A row's disposition and a remainder's statement are propositions with no id
  // of their own, and they were the half the pin skipped: the sentence saying
  // how a displaced route was rejected could be replaced with one that says
  // nothing, and no id, count, or derived status would move.
  refuses(
    run((register) => {
      register.row[0].disposition = "Nothing to see here; reworded to say nothing at all.";
    }),
    "statement pin: row:Fixture displaced route asserts",
  );
  refuses(
    run((register) => {
      register.residue[0].statement = "A remainder restated as something already settled.";
    }),
    `statement pin: residue:${ALLOWED_RESIDUES[0]} asserts`,
  );

  // A negative control's two prose fields are the entire record of what it
  // demonstrated, and they were reachable by an edit that moved no id, no
  // count and no derived status — the one place a hollowing is invisible to
  // this control itself.
  refuses(
    run((register) => {
      register.control[0].observed = "it broke something, exit 1";
    }),
    "statement pin: control:CTL-alpha.observed asserts",
  );
  refuses(
    run((register) => {
      register.control[0].mutation = "Change a thing so the case stops holding.";
    }),
    "statement pin: control:CTL-alpha.mutation asserts",
  );
  // A receiving row's gate is constrained only to OPEN by naming its owner, so
  // everything after that opening was free prose.
  refuses(
    run((register) => {
      const row = register.receiving[0];
      row.gate = `${row.owner_node} acceptance: whatever the owner decides to do.`;
    }),
    `statement pin: receiving:${ALLOWED_RESIDUES[0]}#1.gate asserts`,
  );
  // A skip basis is what turns declared skips into expected ones, so rewriting
  // it rewrites the reason the counter check admits the record. It exists only
  // on a record that declares a skip count, so the pin is exercised against a
  // universe that has one.
  const BASIS = "three cases the suite marks ignored";
  const skipping = (basis) => (register) => {
    Object.assign(register.proof[1], {
      selected: 5,
      executed: 2,
      passed: 2,
      skipped: 3,
      expected_skips: 3,
      skip_basis: basis,
      adapter: "node-runner",
      terminal_summary: "tests 5 | pass 2 | fail 0 | cancelled 0 | skipped 3 | todo 0",
    });
    delete register.proof[1].count_key;
    register.adapter.push({
      id: "node-runner",
      runner: "node",
      argv_prefix: ["--test"],
      summary_shape: "node:test terminal block",
      summary_grammar: "node-test",
      reexecution: "instrument",
    });
    return withSkipBasis("P-beta", BASIS);
  };
  assert.deepEqual(run(skipping(BASIS)).errors, [], "the pinned basis is admissible");
  refuses(
    run(skipping("some cases just do not run here")),
    "statement pin: proof:P-beta.skip_basis asserts",
  );
});

// Pinning what an atom SAYS leaves where it POINTS author-controlled, and the
// two fail differently: a repoint keeps every word, every count and every
// derived status exactly where they are while moving the atom onto whatever a
// green record happens to touch, or onto a contract sentence the contract no
// longer states.
control("an atom repointed at a different artifact or contract sentence is refused", () => {
  covers("repointed-anchor");

  // Relevance. `A-gamma` is anchored at the only artifact `P-gamma` runs, and
  // moving that anchor onto the contract lets `P-alpha` — a real, green,
  // entirely unrelated record — be credited with it. The coverage move ALONE is
  // correctly refused, which is what makes the anchor the load-bearing half.
  refuses(
    run((register) => {
      register.proof[0].covers.push("A-gamma");
      register.proof[2].covers = ["A-plain"];
    }),
    "nothing this record runs or reads reaches tools/fixture-gamma.mjs",
  );
  const repointed = run((register) => {
    register.atom[6].evidence_anchor = "contracts/fixture.md";
    register.proof[0].covers.push("A-gamma");
    register.proof[2].covers = ["A-plain"];
  });
  refuses(repointed, "anchor pin: atom:A-gamma points at");
  assert.deepEqual(
    repointed.errors.filter((error) => error.includes("does not reach")),
    [],
    "with the anchor moved, the relevance gate itself has nothing left to refuse",
  );

  // Contract binding. The atom's pinned statement still describes the sentence
  // it named; repointing it at a surviving sentence leaves that description
  // false with the statement pin silent, because the statement's bytes did not
  // move.
  const rebound = run((register) => {
    register.atom[0].contract_section = "3. Gamma";
    register.atom[0].contract_anchor = "A third obligation with its own section body";
  });
  refuses(rebound, "anchor pin: atom:A-stated points at");
  assert.deepEqual(
    rebound.errors.filter((error) => error.includes("statement pin")),
    [],
    "the statement pin cannot see a repoint, which is why this one exists",
  );

  // Reflowing a quotation is not repointing it: the anchor is matched against
  // flattened prose, so a rewrap changes nothing this validator compares.
  const reflowed = run((register) => {
    register.atom[0].contract_anchor = `${STATED_ANCHOR.replace(" ", "  ")}`;
  });
  assert.deepEqual(
    reflowed.errors.filter((error) => error.includes("anchor pin")),
    [],
    "whitespace is normalised before an anchor is digested",
  );
});

// Breadth is not selection. A lane can consume the whole workspace and then
// narrow to something with no relation to the record, and every other check
// here — the job, the whole command line, the gate, the filter — still passes.
control("a lane narrowed away from the record's own selection does not refresh it", () => {
  covers("unrelated-lane-selection");

  const limitsMatching = ({ model }, fragment) =>
    [...model.proofLimits.values()].flat().filter((limit) => limit.includes(fragment));
  const ARCHIVE = "artifacts/fixture.tar.zst";
  const BUILDER = `cargo nextest archive --workspace --archive-file ${ARCHIVE}`;
  // A lane consuming that archive, with the record itself selecting one package.
  const lane = (command, { builder = BUILDER, needs = "[detect-changes, archive-build]" } = {}) =>
    run((register) => {
      asCargo(register, { refresh_command: command });
      register.proof[0].argv_tail = ["-p", "alpha-crate"];
      return {
        crates: ["alpha-crate"],
        workflow: FIXTURE_WORKFLOW.replace(
          `      - run: ${FIXTURE_REFRESH_COMMAND}`,
          `      - run: ${command}\n\n  archive-build:\n    runs-on: ubuntu-latest\n    steps:\n      - run: ${builder}`,
        ).replace(
          `  ${FIXTURE_REFRESH_JOB}:\n    needs: detect-changes`,
          `  ${FIXTURE_REFRESH_JOB}:\n    needs: ${needs}`,
        ),
      };
    });

  const whole = lane(`cargo nextest run --archive-file ${ARCHIVE}`);
  assert.deepEqual(limitsMatching(whole, "selection"), [], "the unnarrowed lane resolves");

  limits(
    lane(`cargo nextest run --archive-file ${ARCHIVE} -p some-other-crate`),
    "P-alpha",
    "does not include `alpha-crate`",
  );
  limits(
    lane(`cargo nextest run --archive-file ${ARCHIVE} --exclude alpha-crate`),
    "P-alpha",
    "excludes `alpha-crate`",
  );
  limits(
    lane(`cargo nextest run --archive-file ${ARCHIVE} --test only-one`),
    "P-alpha",
    "narrows to the `--test only-one` target",
  );
  assert.equal(
    lane(`cargo nextest run --archive-file ${ARCHIVE} -p some-other-crate`).model.claimStatus.get(
      "CLM-ALPHA",
    ).admissible,
    false,
    "a lane that does not run the record's work is a limit, and a limited claim is inadmissible",
  );

  // A comment or an echo carries the same two substrings as the build command.
  assert.ok(
    limitsMatching(
      lane(`cargo nextest run --archive-file ${ARCHIVE}`, {
        builder: `echo ${BUILDER}\n      - run: "# ${BUILDER}"`,
      }),
      `no job in this workflow builds ${ARCHIVE}`,
    ).length > 0,
    "a line that only mentions the archive does not build it",
  );

  // A build wrapped across a shell line continuation is the same build: the
  // command is what the shell receives, not what one line of YAML holds.
  assert.deepEqual(
    limitsMatching(
      lane(`cargo nextest run --archive-file ${ARCHIVE}`, {
        builder: `cargo nextest archive --workspace \\\n          --archive-file ${ARCHIVE}`,
      }),
      ARCHIVE,
    ),
    [],
    "a producer written over two lines still builds the archive the lane consumes",
  );

  // The dependency edge is a `needs:` declaration, and its block form says
  // exactly what the inline form says.
  assert.deepEqual(
    limitsMatching(
      lane(`cargo nextest run --archive-file ${ARCHIVE}`, {
        needs: "\n      - detect-changes\n      - archive-build",
      }),
      ARCHIVE,
    ),
    [],
    "a block-sequence needs list declares the same dependency the inline form does",
  );

  // A comment between the entries is neither an entry nor the end of the
  // block. Stopping there truncates the list at whatever the author annotated
  // and reports a missing dependency the workflow plainly declares.
  assert.deepEqual(
    limitsMatching(
      lane(`cargo nextest run --archive-file ${ARCHIVE}`, {
        needs: "\n      - detect-changes\n      # the archive producer\n      - archive-build",
      }),
      ARCHIVE,
    ),
    [],
    "an interleaved comment does not truncate a block-sequence needs list",
  );

  // A runner whose selection is positional carries no flag, so "no selection
  // flag" cannot mean "the whole universe".
  assert.ok(
    limitsMatching(
      run((register) => {
        asCargo(register, { refresh_command: "pnpm vitest run tools/one.spec.ts" });
        return {
          workflow: FIXTURE_WORKFLOW.replace(
            FIXTURE_REFRESH_COMMAND,
            "pnpm vitest run tools/one.spec.ts",
          ),
        };
      }),
      "positional operand",
    ).length > 0,
    "a runner handed one file is not re-running its own whole universe",
  );
});

// Breadth and package agreement leave two selections unread: the predicate a
// lane computes at run time, and the list a runner iterates when it takes no
// selection argument at all. Both used to resolve as coverage without anything
// having looked at them, so a lane could be narrowed to nothing related to the
// record while every other check here still passed.
control("a lane selection nothing resolved is not coverage", () => {
  covers("unbound-lane-selection");

  const limitsMatching = ({ model }, fragment) =>
    [...model.proofLimits.values()].flat().filter((limit) => limit.includes(fragment));
  const PRODUCER = "node tools/fixture-filter.mjs";
  // The lane here must cover the fixture's own cited artifacts, so the only
  // limit a case can produce is the one it plants. A `contracts/` input outside
  // the crate-tree filter is a different, already-controlled defect.
  const covering = (workflow) =>
    workflow.replace(
      "            rust:\n              - 'crates/**'",
      "            rust:\n              - 'contracts/**'\n              - 'decisions/**'\n              - 'crates/**'",
    );

  const narrowing = (options = {}) => {
    const {
      command = `cargo nextest run --workspace -E "$FIXTURE_FILTER"`,
      assignment = `export FIXTURE_FILTER="$(${PRODUCER})"`,
      producer = PRODUCER,
      uncoveredByTama = null,
      ...rest
    } = options;
    return run((register) => {
      asCargo(register, {
        refresh_command: command,
        ...(producer === null ? {} : { refresh_selection_producer: producer }),
      });
      register.proof[0].argv_tail = ["-p", "alpha-crate"];
      let workflow = covering(
        FIXTURE_WORKFLOW.replace(
          `      - run: ${FIXTURE_REFRESH_COMMAND}`,
          `      - run: |\n          ${assignment}\n          ${command}`,
        ),
      );
      if (uncoveredByTama)
        workflow = workflow.replace(
          "              - 'tools/**'\n",
          `              - 'tools/**'\n              - '!${uncoveredByTama}'\n`,
        );
      return { crates: ["alpha-crate"], workflow, ...rest };
    });
  };

  const resolved = narrowing();
  assert.deepEqual(resolved.errors, []);
  assert.deepEqual(limitsMatching(resolved, "lane"), [], "a resolvable predicate is not a limit");
  assert.match(
    resolved.model.proofRefresh.get("P-alpha"),
    /predicate resolves to an expression excluding `unrelated-crate`/,
    "the predicate is evaluated and published, not left as a variable name",
  );

  assert.ok(
    limitsMatching(narrowing({ laneFilter: "not package(alpha-crate)" }), "excludes `alpha-crate`")
      .length > 0,
    "a predicate that excludes the record's package does not re-run its work",
  );
  assert.equal(
    narrowing({ laneFilter: "not package(alpha-crate)" }).model.claimStatus.get("CLM-ALPHA")
      .admissible,
    false,
    "an unrefreshed record is a limit, and a limited claim is inadmissible",
  );
  assert.ok(
    limitsMatching(narrowing({ laneFilter: "package(other-crate)" }), "restricts the run to")
      .length > 0,
    "a predicate that positively restricts the run to another package does not run this record",
  );

  // Every leaf of the expression is classified, and every operator is split in
  // the expression language's own precedence. Recognising a predicate head and
  // returning read each of these as "excluding no package, with no test-name
  // narrowing" — full breadth for a lane that selects by name, selects nothing,
  // or moves the package set — which is the same unread-selection defect the
  // variable half of this control refuses, reached through a leaf instead.
  for (const [expression, why] of [
    ["test(/^unrelated::/)", "a positive name-scoped inclusion is not whole-package breadth"],
    ["none()", "a lane that selects nothing is not a lane that re-runs this record"],
    ["deps(other-crate)", "a predicate that moves the package set is not decomposable here"],
    [
      "package(alpha-crate) and none()",
      "one unclassifiable conjunct is enough; the readable half does not carry the answer",
    ],
    // `not` binds tighter than `or`, so this is "not alpha-crate, or
    // other-crate" and NOT "neither". Splitting conjunctions first read the
    // lower-precedence operator as the root and attributed an exclusion to a
    // disjunct that never carried one.
    [
      "not package(alpha-crate) or package(other-crate)",
      "a disjunction under a negated first arm is not a two-package exclusion",
    ],
  ])
    assert.ok(
      limitsMatching(narrowing({ laneFilter: expression }), "a shape this check cannot decompose")
        .length > 0,
      why,
    );
  // The universe itself takes nothing away, so it stays breadth rather than
  // becoming a false resolution failure.
  assert.deepEqual(
    limitsMatching(narrowing({ laneFilter: "all()" }), "lane"),
    [],
    "the whole universe is not a narrowing",
  );

  // A narrowing whose producer is unreadable, wrong, or absent is a resolution
  // FAILURE. Publishing `$FIXTURE_FILTER` and calling that breadth is the shape
  // this refuses: the shell edits below change nothing about what the lane
  // selects, so a check that lost the producer over one of them was reading the
  // keyword rather than the assignment.
  assert.ok(
    limitsMatching(narrowing({ assignment: "# nothing assigns it" }), "$FIXTURE_FILTER").length > 0,
    "a variable no step assigns leaves the lane's narrowing unresolved",
  );
  assert.deepEqual(
    limitsMatching(narrowing({ assignment: `FIXTURE_FILTER="$(${PRODUCER})"` }), "lane"),
    [],
    "dropping the export keyword changes nothing about where the selection comes from",
  );
  assert.deepEqual(
    limitsMatching(
      narrowing({
        assignment: `export FIXTURE_FILTER="$(node \\\n            tools/fixture-filter.mjs)"`,
      }),
      "lane",
    ),
    [],
    "an assignment wrapped across a shell line continuation is still the assignment",
  );
  assert.ok(
    limitsMatching(
      narrowing({ producer: "node tools/fixture-gamma.mjs" }),
      "not the declared `node tools/fixture-gamma.mjs`",
    ).length > 0,
    "the command this check evaluates must be the one the register declares",
  );
  refuses(
    narrowing({ producer: null }),
    "its lane narrows at run time by a variable, so the command that computes it must be declared",
  );

  // The selector this check EXECUTES is an entry file, and the value the view
  // publishes is declared one import away from it. Citing the entry alone left
  // the module the expression actually comes from outside the instrument's own
  // trigger paths, so editing it changes the published narrowing on a pull
  // request this lane is not eligible on — the break merges green there and
  // surfaces later on an unrelated roadmap change. The whole first-party import
  // graph of the selector is therefore cited with it.
  refuses(
    narrowing({ uncoveredByTama: "tools/fixture-filter-internals.mjs" }),
    "tools/fixture-filter-internals.mjs is cited as evidence but no tama trigger path covers it",
  );
  assert.deepEqual(
    narrowing({ uncoveredByTama: "tools/nothing-imports-this.mjs" }).errors,
    [],
    "the citation is the selector's own import graph, not every file under tools",
  );

  // The same graph decides whether the selector can RUN where this check runs.
  // The job hosting the instrument installs no dependencies, so a selector that
  // reached an installed package would resolve on a tree that happens to have
  // one and fail in the job that has to answer.
  assert.ok(
    limitsMatching(narrowing({ laneFilterImports: "yaml" }), "reaches the installed package `yaml`")
      .length > 0,
    "a lane selector that reaches an installed package is not resolvable here",
  );

  // The other unread selection: a runner iterating its own list. The lane
  // command names no package and no operand, which used to resolve as "its own
  // default universe" — a sentence about a list nothing had read.
  const enumerated = (options = {}) => {
    const {
      enumerator = "node tools/fixture-owners.mjs --list",
      selection = "alpha",
      command = "node tools/fixture-owners.mjs",
      uncoveredByTama = null,
      ...rest
    } = options;
    const withoutTamaCoverage = (workflow) =>
      uncoveredByTama
        ? workflow.replace(
            "              - 'tools/**'\n",
            `              - 'tools/**'\n              - '!${uncoveredByTama}'\n`,
          )
        : workflow;
    return run((register) => {
      asCargo(register, {
        refresh_command: command,
        ...(enumerator === null
          ? {}
          : { refresh_selection_enumerator: enumerator, refresh_selection_flag: "--features" }),
      });
      register.adapter[0].argv_prefix = ["run", "--features", selection];
      for (const proof of register.proof) proof.argv_tail = [];
      register.claim[2].subject = ["decisions/fixture.md"];
      register.atom[6].evidence_anchor = "contracts/fixture.md";
      for (const proof of register.proof) proof.fixtures = ["contracts/fixture.md"];
      return withAnchors(register, {
        workflow: withoutTamaCoverage(
          covering(FIXTURE_WORKFLOW.replace(FIXTURE_REFRESH_COMMAND, command)),
        ),
        ...rest,
      });
    });
  };

  const listed = enumerated();
  assert.deepEqual(listed.errors, []);
  assert.match(
    listed.model.proofRefresh.get("P-alpha"),
    /lists 2 entries including this record's `--features alpha`/,
    "the enumerated universe is read and the record's own selection resolved against it",
  );
  assert.ok(
    limitsMatching(enumerated({ laneOwners: ["beta", "gamma"] }), "does not include this record's")
      .length > 0,
    "dropping the record's owner from the list the lane iterates stops the lane refreshing it",
  );
  assert.ok(
    limitsMatching(enumerated({ enumerator: null }), "declares no enumeration").length > 0,
    "an unread default universe is not coverage",
  );
  assert.ok(
    limitsMatching(
      enumerated({ enumerator: "node tools/fixture-gamma.mjs --list" }),
      "is not `node tools/fixture-owners.mjs` plus a single listing flag",
    ).length > 0,
    "the enumeration must be a run of the script the lane itself runs",
  );
  // Same script, different arguments. Matching the program alone accepted any
  // argument vector after it, so a list printed under a selection the lane
  // never issues would have been read as the list the lane iterates. The
  // enumeration is this lane's own command line plus one switch, nothing else.
  assert.ok(
    limitsMatching(
      enumerated({ enumerator: "node tools/fixture-owners.mjs --list --only beta" }),
      "plus a single listing flag",
    ).length > 0,
    "an enumeration under arguments the lane does not issue is not the lane's own universe",
  );
  // Same hole on this arm: the enumerator is an entry, and the list it prints
  // is declared in the module it imports.
  refuses(
    enumerated({ uncoveredByTama: "tools/fixture-owners-internals.mjs" }),
    "tools/fixture-owners-internals.mjs is cited as evidence but no tama trigger path covers it",
  );

  // Portability, on the enumerated half as on the predicate half. The job that
  // runs this instrument installs nothing, so an enumeration reaching an
  // installed package resolves on a tree that happens to have it and fails in
  // the job that has to answer — a resolution that holds by accident is the
  // same unread selection this control refuses everywhere else.
  assert.ok(
    limitsMatching(
      enumerated({ laneOwnersImports: "yaml" }),
      "reaches the installed package `yaml`",
    ).length > 0,
    "a lane enumeration that reaches an installed package is not resolvable here",
  );
});

// An obligation the shipped code does not meet is a remainder, and it leaves
// the way every other remainder does.
//
// Two failure shapes are discriminated here. Marking one beside a COVERED atom
// left the claim deriving PROVEN, so a requirement known not to hold reached
// the summary row under the status of one that holds — a fourth remainder in
// substance, authorised by nothing, while the closed residue set exists to make
// a fourth one unavailable. And routing one to a receiving sequence none of
// whose owners may touch the surface that has to change is a carry to nobody,
// which no set-membership, descendant, or role check can see.
control("an owed obligation is a remainder, and one carried to nobody is refused", () => {
  covers("owed-obligation-unowned");

  const OWED = "crates/fixture-owned/src/lib.rs";
  // `A-baseline` is the fixture's transferred atom, so an obligation marked on
  // it leaves through an approved transfer to an admissible residue.
  const owed = (options = {}) => {
    const { surface = OWED, atom = "A-baseline", ...rest } = options;
    return run((register) => {
      register.atom.find((row) => row.id === atom).shipped_obligation = surface;
      return withAnchors(register, rest);
    });
  };

  const carried = owed();
  assert.deepEqual(carried.errors, []);
  assert.deepEqual(
    {
      surface: carried.model.owedAtoms.get("A-baseline").surface,
      residue: carried.model.owedAtoms.get("A-baseline").residue,
      owner: carried.model.owedAtoms.get("A-baseline").owner,
    },
    { surface: OWED, residue: ALLOWED_RESIDUES[2], owner: "TCM1" },
    "the carry is derived — which remainder it leaves through, and which receiving owner may change the surface",
  );
  // The lifecycle, not a column: the row a reader scans cannot say PROVEN while
  // one of the claim's obligations is known not to hold.
  assert.equal(claimStatus(carried.model, "CLM-BETA"), PROVEN_BOUNDED);
  assert.match(
    renderView(carried.model),
    /\| `CLM-BETA` \| PROVEN-BOUNDED \| 4 \| 1 \| 1 \| 3 \|/u,
    "the claim summary a reader scans carries the owed count beside a status the carry already moved",
  );

  // The route itself. An obligation marked on an atom this block proves has
  // taken no remainder, no approval artifact, and no ordered receiving rows.
  refuses(
    owed({ atom: "A-plain" }),
    "must leave through an approved transfer to an admissible residue, not through a status column",
  );
  refuses(
    owed({ downstreamSurfaces: ["crates/somewhere-else/src"] }),
    `declares a production surface containing ${OWED}`,
  );
  refuses(owed({ surface: "crates/fixture-owned/src/absent.rs" }), "owed surface does not resolve");
  refuses(
    run((register) => {
      register.atom.find((row) => row.id === "A-baseline").received_by = "TCM1-AC1";
      register.atom.find((row) => row.id === "A-baseline").received_by_role = "sole-owner outcome";
      return withAnchors(register);
    }),
    "transferred atoms are received through their residue, not received_by",
  );

  // The route around all of it: say nothing. While the declaration was
  // optional, an atom could disclose an unmet composition in its own statement,
  // omit the field, keep its quotation coverage and derive PROVEN — every check
  // above skipped, because the first thing they read was absent. So the
  // declaration is required, and the only path to a met status is a positive
  // claim about bytes rather than a silence.
  refuses(
    run((register) => {
      delete register.atom.find((row) => row.id === "A-baseline").shipped_obligation;
      return withAnchors(register);
    }),
    "missing required property shipped_obligation",
  );
  refuses(owed({ surface: "   " }), "every atom must declare what the shipped code owes it");

  // And the route BESIDE all of it: say the wrong one of the three named
  // values. Each is a contradiction against something the register already
  // carries, so none of them is an author's word against itself.
  //
  // `met` on a transferred atom was the live shape this closes: the field was
  // applied mechanically as met across every atom, three of which are the
  // register's own open remainders, and "the shipped code meets this" and "this
  // is an open question for a successor" were both asserted about one atom with
  // nothing deriving the contradiction.
  refuses(
    owed({ surface: SHIPPED_OBLIGATION_MET }),
    "declares the shipped code meets it while leaving through",
  );
  refuses(
    owed({ surface: SHIPPED_OBLIGATION_AUTHORITY_ONLY }),
    `carried to ${ALLOWED_RESIDUES[2]} is "${SHIPPED_OBLIGATION_CARRIED}", not exempt from the question`,
  );
  refuses(
    owed({ atom: "A-plain", surface: SHIPPED_OBLIGATION_CARRIED }),
    "no approved transfer carries it",
  );
  // The exemption is not an author's to grant either: an atom anchored INSIDE a
  // production surface this node declares has shipped code as its subject, so
  // "there is no code this applies to" is false about the register's own bytes.
  refuses(
    run((register) => {
      const atom = register.atom.find((row) => row.id === "A-plain");
      atom.evidence_anchor = "crates/fixture-owned/src/lib.rs";
      atom.shipped_obligation = SHIPPED_OBLIGATION_AUTHORITY_ONLY;
      return withAnchors(register);
    }),
    "inside a production surface TCM0R declares",
  );

  // A citation is only a citation once it says which criterion role the
  // receiving block bears; without one, nothing about the hand-off resolves.
  refuses(
    run((register) => {
      delete register.atom[0].received_by_role;
    }),
    "a criterion citation must declare one of",
  );

  // And the block it names has to exist in the DAG: an interchangeable ordinal
  // on no node is an obligation carried to nobody.
  refuses(
    run((register) => {
      register.atom[0].received_by = "ZZZZ-AC1";
    }),
    "is not a DAG node",
  );

  // The remainder exists and is admissible, but no ordered receiving row names
  // a block for it. The obligation then leaves through a route that ends
  // nowhere, which reads exactly like a carried one in the summary.
  refuses(
    run((register) => {
      register.atom.find((row) => row.id === "A-baseline").shipped_obligation = OWED;
      register.receiving = register.receiving.filter((row) => row.residue !== ALLOWED_RESIDUES[2]);
      return withAnchors(register, {});
    }),
    "declares no receiving owner",
  );

  // The receiving owner is named and its charter is not in the tree, so
  // whether that block may change the surface cannot be derived at all. An
  // underivable answer is not a passing one.
  refuses(owed({ omitCharter: "TCM1" }), "has no resolvable charter");
});

// The acyclicity rule reads the artifacts a record's command RUNS. A suite that
// imports its claim's subject and calls it is executing that subject, which the
// producer derivation cannot see, so that edge is derived separately and
// refused by default.
control(
  "a record that executes its claim's subject is refused unless the exemption is pinned",
  () => {
    covers("self-executing-subject");

    const importsSubject = (extra = {}) =>
      run((register) => {
        register.claim[2].subject = [FIXTURE_SUBJECT];
        return {
          gammaText: `import { alpha } from "./fixture-subject.mjs";\n\nconsole.log(alpha);\n`,
          ...extra,
        };
      });

    refuses(importsSubject(), `reaches ${FIXTURE_SUBJECT} through its first-party import graph`);
    assert.equal(
      claimStatus(importsSubject().model, "CLM-GAMMA"),
      "REFUSED",
      "an unpinned self-executing record refuses its claim rather than being logged beside it",
    );

    const pinned = importsSubject({
      exercising: { "P-gamma": "its cases pass only when the subject REFUSES a planted mutation" },
    });
    assert.deepEqual(pinned.errors, []);
    assert.match(
      pinned.model.proofRefresh.get("P-gamma"),
      /exercises its claim's subject/,
      "a pinned exemption is published beside the record rather than kept in the validator",
    );

    // One re-export is enough to hide the whole edge. `fixture-gamma` imports a
    // shim, the shim imports the subject, and a reader that stopped at the entry
    // file's own specifiers reports the shim, derives no cycle, and requires no
    // exemption — while the record's run executes the subject exactly as before.
    const throughShim = run((register) => {
      register.claim[2].subject = [FIXTURE_SUBJECT];
      return {
        gammaText: `import { alpha } from "./fixture-shim.mjs";\n\nconsole.log(alpha);\n`,
        shimText: `export { alpha } from "./fixture-subject.mjs";\n`,
      };
    });
    refuses(throughShim, `reaches ${FIXTURE_SUBJECT} through its first-party import graph`);
    assert.equal(
      claimStatus(throughShim.model, "CLM-GAMMA"),
      REFUSED,
      "a cycle reached through a forwarding module is the same cycle",
    );

    // The walk terminates on a graph with a cycle in it, which a module graph is
    // allowed to have.
    const mutual = run((register) => {
      register.claim[2].subject = ["tools/fixture-shim.mjs"];
      return {
        gammaText: `import { alpha } from "./fixture-shim.mjs";\n\nconsole.log(alpha);\n`,
        shimText: `import "./fixture-gamma.mjs";\n\nexport const alpha = 1;\n`,
      };
    });
    refuses(mutual, "reaches tools/fixture-shim.mjs through its first-party import graph");

    // A carve-out nobody needs is a carve-out nobody reviews.
    refuses(
      run(() => ({ exercising: { "P-alpha": "a reason for something that no longer happens" } })),
      "proof P-alpha: pinned as exercising its claim's subject",
    );
  },
);

selfAssertion("the registered controls are exactly the declared mandatory classes", () => {
  assert.deepEqual(
    [...SELF_ASSERTIONS].sort(),
    [...DECLARED_SELF_ASSERTIONS].sort(),
    "a case that asserts over this suite's declarations rather than over a planted mutation must be named here, because the exemption that admits this suite's import of its own subject does not cover it",
  );
  assert.deepEqual([...REGISTERED].sort(), [...MANDATORY_CONTROL_CLASSES].sort());
});

selfAssertion("the closed universes are pinned, not derived from the register", () => {
  assert.equal(new Set(MUST_CLOSE_FINDINGS).size, MUST_CLOSE_FINDINGS.length);
  assert.equal(MUST_CLOSE_FINDINGS.length, 32);
  assert.deepEqual(Object.keys(RESIDUE_FINDINGS).sort(), ["AD4", "C2", "C7", "C9"]);
  assert.deepEqual([...ALLOWED_RESIDUES].sort(), [
    "TCM0-R-HANG-TOPOLOGY",
    "TCM0-R-IMPLEMENTATION-BASELINE",
    "TCM0-R-TOPOLOGY-SELECTION",
  ]);
  for (const residue of Object.values(RESIDUE_FINDINGS))
    assert.ok(ALLOWED_RESIDUES.includes(residue));

  // The claim and atom universes are pinned the same way, and every atom
  // belongs to exactly one claim.
  const pinned = Object.values(LIVE_UNIVERSE.claims).flat();
  assert.equal(new Set(pinned).size, pinned.length, "an atom is pinned under two claims");
  assert.ok(pinned.length > 0);
  for (const kind of ["deletion", "survivor"])
    assert.ok(LIVE_UNIVERSE.rows[kind].length > 0, `${kind} rows are pinned`);
});

// --- the live register ------------------------------------------------------

selfAssertion(
  "the live register validates, is reviewable, and never derives an admissible-by-fiat state",
  () => {
    const analysis = analyze(PACKAGE_ROOT);
    const { errors, model } = analysis;
    assert.deepEqual(errors, []);
    assert.equal(model.state, READY_FOR_REVIEW);
    assert.equal(certification(analysis).ok, true);
    for (const [id, derived] of model.claimStatus) {
      assert.notEqual(derived.status, OPEN, `${id} is uncovered: ${derived.uncovered.join(", ")}`);
      assert.notEqual(derived.status, REFUSED, `${id} is refused`);
      assert.equal(derived.admissible, true, `${id} is bounded without an approved transfer`);
    }
    for (const [id, limits] of model.proofLimits)
      assert.deepEqual(limits, [], `${id} carries a derived limit`);
    assert.deepEqual(
      model.register.finding.map((finding) => finding.id).sort(),
      [...MUST_CLOSE_FINDINGS, ...Object.keys(RESIDUE_FINDINGS)].sort(),
    );
    assert.equal(viewIsFresh(PACKAGE_ROOT, model), true, "regenerate the view with --write");
    assert.ok(!renderView(model).includes("ADMISSIBLE"));
  },
);

// A transcribed counter is only as good as the last time somebody ran the
// command. These two records are about artifacts this suite can observe
// directly, so they are bound rather than trusted: a case added here, or a row
// added to the register, invalidates the transcription until it is re-run.
selfAssertion("the transcribed counters for observable records still match reality", () => {
  const { model } = analyze(PACKAGE_ROOT);
  const proofs = new Map(model.register.proof.map((proof) => [proof.id, proof]));

  const suite = proofs.get("P-instrument-controls");
  assert.equal(
    suite.selected,
    DECLARED_CASES,
    `this suite declares ${DECLARED_CASES} cases; the record claims ${suite.selected}`,
  );
  assert.equal(suite.executed, DECLARED_CASES);
  assert.equal(suite.passed, DECLARED_CASES);

  const register = model.register;
  const expected =
    `claims=${register.claim.length} atoms=${register.atom.length} proofs=${register.proof.length}` +
    ` controls=${register.control.length} findings=${register.finding.length}` +
    ` residues=${register.residue.length} obligations=${model.obligations} state=${model.state}`;
  assert.ok(
    proofs.get("P-instrument-check").terminal_summary.endsWith(expected),
    `the recorded validator summary no longer matches the register; expected it to end with:\n${expected}`,
  );
});
