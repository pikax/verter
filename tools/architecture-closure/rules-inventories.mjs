// Engine relocation, semantic-observe and typed-reuse inventory rules.

const OBSERVE_FILE = /^crates\/[^/]+\/observe-inventory\/([^/]+)\.md$/;
const OBSERVE_FIELDS = ["productionConsumer", "classification", "lifetime", "ownerModule", "gate"];
const REFUSALS = new Set(["OperationRefusal", "IsolatedRootLedgerRefusal"]);

function checkTestMapping(ctx, mapping) {
  const incomplete = (subject, message) => ctx.add("engine-inventory-incomplete", subject, message);
  if (!mapping?.rows?.length) {
    incomplete("testMapping", "no old-to-new discovered-test mapping");
    return;
  }
  const olds = new Set(mapping.rows.map((row) => row.old));
  if (olds.size !== mapping.rows.length) incomplete("testMapping", "a test is mapped twice");
  if (mapping.discoveredBefore !== mapping.rows.length) {
    incomplete("testMapping", "the mapping does not cover every discovered test");
  }
  if (
    mapping.discoveredAfter !== mapping.discoveredBefore ||
    mapping.normalizedIdentical !== true
  ) {
    incomplete("testMapping", "the normalised test population changed");
  }
}

function checkGuards(ctx, guards) {
  const incomplete = (subject, message) => ctx.add("engine-inventory-incomplete", subject, message);
  const kinds = new Set(guards?.map((guard) => guard.kind));
  for (const kind of ctx.policy.engineGuardKinds) {
    if (!kinds.has(kind)) incomplete(`guards/${kind}`, `no ${kind} enforcement family`);
  }
  for (const guard of guards ?? []) {
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
  if (!engine?.relocations?.length) incomplete("relocations", "no relocation inventory");
  checkTestMapping(ctx, engine?.testMapping);
  checkGuards(ctx, engine?.guards);
  const updated = new Set(
    engine?.formatInventories?.filter((row) => row.updated).map((row) => row.id),
  );
  for (const id of ctx.policy.engineFormatInventories) {
    if (!updated.has(id)) incomplete(`formatInventories/${id}`, "not updated with the moves");
  }
  if (!engine?.futureCharterSurfaces?.length) {
    incomplete("futureCharterSurfaces", "no affected future surface is named");
  }
}

function checkObserveFile(ctx, file) {
  const incomplete = (subject, message) =>
    ctx.add("observe-inventory-incomplete", subject, message);
  if (OBSERVE_FILE.exec(file.path)?.[1] !== file.owner) {
    incomplete(file.path, `not the per-owner inventory file of ${file.owner}`);
  }
  if (!file.rows?.length) incomplete(file.path, "no inventory rows");
  for (const row of file.rows ?? []) {
    const subject = `${file.owner}/${row.item}`;
    for (const field of OBSERVE_FIELDS) {
      if (row[field] === undefined) incomplete(subject, `no ${field}`);
    }
    if (row.classification === "OPTIONAL" && /^`?always\b/.test(row.gate ?? "")) {
      ctx.add(
        "optional-default-hot-bookkeeping",
        subject,
        "optional state is on in default builds",
      );
    }
  }
}

function checkObserveInventory(ctx, observe) {
  if (!observe?.files?.length) {
    ctx.add("observe-inventory-incomplete", "files", "no per-owner inventory files");
  }
  for (const file of observe?.files ?? []) checkObserveFile(ctx, file);
  const counts = new Map(observe?.counts?.map((row) => [row.name, row]));
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

function checkAdmission(ctx, admission) {
  const subject = admission.id;
  const refusal = REFUSALS.has(admission.class) || admission.result === "Refused";
  if (refusal && admission.reusedAs === "complete") {
    ctx.add("refusal-as-complete", subject, "a retained refusal was served as a completed answer");
  }
  if (admission.reusedAs === "none") return;
  const incomplete = admission.result === "Approximate" || admission.result === "Unavailable";
  if (admission.class === "Never" || incomplete) {
    ctx.add("arbitrary-incomplete-reuse", subject, "only admitted classes may be reused");
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
  for (const admission of reuse.admissions ?? []) checkAdmission(ctx, admission);
  for (const dimension of ctx.policy.negativeCacheDimensions) {
    const proofs = reuse.negativeCacheTests?.filter((row) => row.dimension === dimension) ?? [];
    if (!proofs.some((row) => row.executedTests > 0 && row.passed === true)) {
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
