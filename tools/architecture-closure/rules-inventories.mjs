// Engine relocation, semantic-observe and typed-reuse inventory rules. Each
// inventory population is declared by the export; the report's rows are
// compared against it, so a report can neither omit a declared row (the one
// that would fail) nor complete an inventory by listing fewer rows.

const OBSERVE_FILE = /^crates\/[^/]+\/observe-inventory\/([^/]+)\.md$/;
const OBSERVE_FIELDS = ["productionConsumer", "classification", "lifetime", "ownerModule", "gate"];
const REFUSALS = new Set(["OperationRefusal", "IsolatedRootLedgerRefusal"]);

/**
 * Matches report rows to the export-declared population: each declared entry
 * needs exactly one row, and a row the export does not declare is rejected.
 * `declaredKeyOf` names a declared entry when its shape differs from a row's.
 * Returns the matched pairs `[declared, row]`.
 */
function matchDeclared(ctx, { declared, rows, keyOf, declaredKeyOf = keyOf, code, label }) {
  const indexed = ctx.index(rows, keyOf, code);
  const out = [];
  const known = new Set();
  for (const entry of declared) {
    const key = declaredKeyOf(entry);
    known.add(key);
    const row = indexed.get(key);
    if (row) out.push([entry, row]);
    else ctx.add(code, key, `the declared ${label} has no report row`);
  }
  for (const key of indexed.keys()) {
    if (!known.has(key)) ctx.add(code, key, `not a declared ${label}`);
  }
  return out;
}

/**
 * The old-to-new mapping must be a bijection between the export's discovered
 * test populations: every test discovered before the moves maps to exactly one
 * test discovered after them, and every discovered destination is reached once.
 * The report's counts are claims and must agree with those populations.
 */
function checkTestMapping(ctx, mapping) {
  const incomplete = (subject, message) => ctx.add("engine-inventory-incomplete", subject, message);
  const { engineTestsBefore: before, engineTestsAfter: after } = ctx.graph.closure;
  if (mapping === undefined) {
    incomplete("testMapping", "no old-to-new discovered-test mapping");
    return;
  }
  matchDeclared(ctx, {
    declared: before,
    rows: mapping.rows,
    keyOf: (row) => `testMapping/old/${row.old}`,
    declaredKeyOf: (id) => `testMapping/old/${id}`,
    code: "engine-inventory-incomplete",
    label: "test discovered before the moves",
  });
  matchDeclared(ctx, {
    declared: after,
    rows: mapping.rows,
    keyOf: (row) => `testMapping/new/${row.new}`,
    declaredKeyOf: (id) => `testMapping/new/${id}`,
    code: "engine-inventory-incomplete",
    label: "test discovered after the moves",
  });
  if (mapping.discoveredBefore !== before.length || mapping.discoveredAfter !== after.length) {
    incomplete("testMapping", "the discovered counts disagree with the export's populations");
  }
  if (before.length !== after.length || mapping.normalizedIdentical !== true) {
    incomplete("testMapping", "the normalised test population changed");
  }
}

function checkGuards(ctx, guards) {
  const incomplete = (subject, message) => ctx.add("engine-inventory-incomplete", subject, message);
  const declared = ctx.graph.closure.engineGuardFamilies;
  const declaredKinds = new Set(declared.map((family) => family.kind));
  for (const kind of ctx.policy.engineGuardKinds) {
    if (!declaredKinds.has(kind)) {
      ctx.add(
        "plan-field-missing",
        `closure.engineGuardFamilies/${kind}`,
        `the export declares no ${kind} enforcement family`,
      );
    }
  }
  const matched = matchDeclared(ctx, {
    declared,
    rows: guards,
    keyOf: (guard) => `guards/${guard.family}`,
    code: "engine-inventory-incomplete",
    label: "enforcement family",
  });
  for (const [family, guard] of matched) {
    if (guard.kind !== family.kind) {
      incomplete(`guards/${guard.family}`, `declared ${family.kind}, got ${guard.kind}`);
    }
    if (guard.path === undefined || guard.plantedFixture === undefined) {
      incomplete(`guards/${guard.family}`, "names no location or planted fixture");
    }
    if (!(guard.executedTests > 0) || guard.plantedRejected !== true) {
      ctx.add(
        "moved-guard-not-executing",
        guard.family,
        "no execution rejecting its planted fixture",
      );
    }
  }
}

function checkEngineInventory(ctx, engine) {
  const incomplete = (subject, message) => ctx.add("engine-inventory-incomplete", subject, message);
  const { closure } = ctx.graph;
  matchDeclared(ctx, {
    declared: closure.engineRelocations,
    rows: engine?.relocations,
    keyOf: (row) => `relocations/${row.from} -> ${row.to}`,
    code: "engine-inventory-incomplete",
    label: "relocation",
  });
  checkTestMapping(ctx, engine?.testMapping);
  checkGuards(ctx, engine?.guards);
  const formats = ctx.index(
    engine?.formatInventories,
    (row) => `formatInventories/${row.id}`,
    "engine-inventory-incomplete",
  );
  for (const id of ctx.policy.engineFormatInventories) {
    if (formats.get(`formatInventories/${id}`)?.updated !== true) {
      incomplete(`formatInventories/${id}`, "not updated with the moves");
    }
  }
  const surfaces = matchDeclared(ctx, {
    declared: closure.futureCharterSurfaces,
    rows: engine?.futureCharterSurfaces,
    keyOf: (row) => `futureCharterSurfaces/${row.surface}`,
    code: "engine-inventory-incomplete",
    label: "affected future surface",
  });
  for (const [entry, row] of surfaces) {
    if (row.owner !== entry.owner) {
      incomplete(`futureCharterSurfaces/${row.surface}`, `declared owner ${entry.owner}`);
    }
  }
}

/**
 * A per-owner file's rows must be exactly the items the export declares for
 * it, with the declared classification and gate, so a report can neither drop
 * the row that would fail nor reclassify or regate it.
 */
function checkObserveFile(ctx, declared, file) {
  const incomplete = (subject, message) =>
    ctx.add("observe-inventory-incomplete", subject, message);
  if (OBSERVE_FILE.exec(file.path)?.[1] !== file.owner) {
    incomplete(file.path, `not the per-owner inventory file of ${file.owner}`);
  }
  const rows = matchDeclared(ctx, {
    declared: declared.items,
    rows: file.rows,
    keyOf: (row) => `${file.owner}/${row.item}`,
    code: "observe-inventory-incomplete",
    label: `item of ${file.owner}`,
  });
  for (const [item, row] of rows) {
    const subject = `${file.owner}/${row.item}`;
    for (const field of OBSERVE_FIELDS) {
      if (row[field] === undefined) incomplete(subject, `no ${field}`);
    }
    for (const field of ["classification", "gate"]) {
      if (row[field] !== undefined && row[field] !== item[field]) {
        incomplete(subject, `declared ${field} ${item[field]}, got ${row[field]}`);
      }
    }
  }
}

/**
 * Optional state that is on in default builds, judged on the export's declared
 * items so the verdict never depends on what the report transcribes.
 */
function checkDefaultHotBookkeeping(ctx) {
  for (const { owner, items } of ctx.graph.closure.observeOwners) {
    for (const item of items) {
      if (item.classification === "OPTIONAL" && /^`?always\b/.test(item.gate)) {
        ctx.add(
          "optional-default-hot-bookkeeping",
          `${owner}/${item.item}`,
          "optional state is on in default builds",
        );
      }
    }
  }
}

function checkObserveInventory(ctx, observe) {
  checkDefaultHotBookkeeping(ctx);
  const files = matchDeclared(ctx, {
    declared: ctx.graph.closure.observeOwners,
    rows: observe?.files,
    keyOf: (file) => file.owner,
    code: "observe-inventory-incomplete",
    label: "per-owner inventory file",
  });
  for (const [declared, file] of files) {
    if (file.path !== declared.path) {
      ctx.add("observe-inventory-incomplete", file.path, `the declared file is ${declared.path}`);
    }
    checkObserveFile(ctx, declared, file);
  }
  const counts = ctx.index(observe?.counts, (row) => row.name, "wsp6-count-duplicate");
  for (const name of ctx.graph.closure.wsp6Counts) {
    const count = counts.get(name);
    if (!count || count.accessor === undefined) {
      ctx.add("wsp6-count-absent", name, "a required count has no production accessor");
    } else if (count.availableWithCaptureOff !== true) {
      ctx.add("wsp6-count-requires-capture", name, "a required count must read with capture off");
    }
  }
}

/** The inventories both modes validate. */
export function checkStructuralInventories(ctx) {
  checkEngineInventory(ctx, ctx.report.inventories.engine);
  checkObserveInventory(ctx, ctx.report.inventories.observe);
}

function checkAdmission(ctx, declared, admission) {
  const subject = admission.id;
  if (admission.class !== declared.class) {
    ctx.add(
      "reuse-inventory-incomplete",
      subject,
      `declared ${declared.class}, got ${admission.class}`,
    );
  }
  const refusal = REFUSALS.has(admission.class) || admission.result === "Refused";
  if (refusal && admission.reusedAs === "complete") {
    ctx.add("refusal-as-complete", subject, "a retained refusal was served as a completed answer");
  }
  if (admission.reusedAs === "none") return;
  const incomplete = admission.result === "Approximate" || admission.result === "Unavailable";
  if (admission.class === "Never" || incomplete) {
    ctx.add("arbitrary-incomplete-reuse", subject, "only admitted classes may be reused");
  } else if (admission.reusedAs === "refusal" && !REFUSALS.has(admission.class)) {
    ctx.add(
      "context-poisoned-reuse",
      subject,
      "only scoped operation and isolated-root ledger refusals may be reused as refusals",
    );
  }
  if (admission.class === "OperationRefusal" && admission.profileScoped !== true) {
    ctx.add("context-poisoned-reuse", subject, "an operation refusal must be profile-scoped");
  }
  if (admission.class === "IsolatedRootLedgerRefusal") {
    const scoped =
      admission.profileScoped === true &&
      admission.isolatedRoot === true &&
      admission.contextKeyed === true;
    if (!scoped) {
      ctx.add(
        "context-poisoned-reuse",
        subject,
        "a ledger refusal must be an isolated, keyed root",
      );
    }
  }
}

/** Typed-reuse truthfulness, the observe stripped control and negative-cache coverage. */
export function checkClosureInventories(ctx) {
  const { reuse, observe } = ctx.report.inventories;
  if (observe?.offVersusStripped === undefined) {
    ctx.add(
      "observe-off-versus-stripped-missing",
      "offVersusStripped",
      "no stripped-control evidence",
    );
  } else if (!ctx.evidenceById.has(observe.offVersusStripped)) {
    ctx.add(
      "evidence-ref-dangling",
      "offVersusStripped",
      `no evidence row ${observe.offVersusStripped}`,
    );
  }
  if (reuse === undefined) {
    ctx.add("reuse-inventory-missing", "reuse", "the report has no typed-reuse inventory");
    return;
  }
  const admissions = matchDeclared(ctx, {
    declared: ctx.graph.closure.reuseAdmissions,
    rows: reuse.admissions,
    keyOf: (admission) => admission.id,
    code: "reuse-inventory-incomplete",
    label: "reuse admission",
  });
  for (const [declared, admission] of admissions) checkAdmission(ctx, declared, admission);
  for (const dimension of ctx.policy.negativeCacheDimensions) {
    const proofs = reuse.negativeCacheTests?.filter((row) => row.dimension === dimension) ?? [];
    // Every row of a dimension must pass: a failing proof beside a passing one still fails.
    const proven = (row) => row.executedTests > 0 && row.passed === true;
    if (proofs.length === 0 || !proofs.every(proven)) {
      ctx.add("negative-cache-test-missing", dimension, "no executed passing negative-cache test");
    }
  }
  if (reuse.perFactTransportExact !== true) {
    ctx.add("per-fact-transport-inexact", "reuse", "per-fact transport is not proven exact");
  }
  if (reuse.unownedGraphEscapes !== 0) {
    ctx.add("unowned-graph-escape", "reuse", "expected exactly zero unowned graph escapes");
  }
}
