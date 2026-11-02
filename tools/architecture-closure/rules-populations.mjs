// Review, capability, metric, evidence and exception population rules.

/** Per-train cumulative review coverage over the trains of the required owners. */
export function checkTrainReviews(ctx) {
  const { graph, policy, report, target } = ctx;
  const members = new Map();
  for (const slug of graph.ancestorsOf(target)) {
    const task = graph.task(slug);
    if (!members.has(task.train)) members.set(task.train, []);
    members.get(task.train).push(task);
  }
  const reviews = ctx.index(report.trainReviews, (row) => row.train, "train-review-duplicate");
  for (const [train, tasks] of members) {
    const review = reviews.get(train);
    if (!review) {
      ctx.add("train-review-missing", train, "a required train has no cumulative review");
      continue;
    }
    if (review.candidate !== report.candidate) {
      ctx.add("train-review-stale", train, "the review does not cover this candidate");
    }
    const covered = new Set(review.checkpoints ?? []);
    for (const task of tasks) {
      if (task.labels.includes(policy.checkpointLabel) && !covered.has(task.slug)) {
        ctx.add("train-checkpoint-uncovered", train, `checkpoint ${task.slug} is not covered`);
      }
    }
    for (const finding of review.findings ?? []) {
      if (finding.status === "deferred") {
        ctx.add("train-review-open-deferral", train, `finding ${finding.id} is still deferred`);
      } else if (finding.status === "open") {
        ctx.add(
          "train-review-unresolved-finding",
          train,
          `${finding.severity} ${finding.id} is open`,
        );
      }
    }
  }
  for (const train of policy.independentReviewTrains) {
    if (reviews.get(train)?.independent !== true) {
      ctx.add("independent-review-missing", train, "no independent cumulative review");
    }
  }
  const terminalTrain = graph.task(target).train;
  if (reviews.get(terminalTrain)?.terminal !== true) {
    ctx.add("terminal-review-missing", terminalTrain, "no terminal architecture review");
  }
}

function checkCapabilityCases(ctx, surface, row) {
  const cases = ctx.index(
    row.cases,
    (entry) => `${surface.surface}/${entry.case}`,
    "capability-cell-duplicate",
  );
  for (const declared of surface.cases) {
    const subject = `${surface.surface}/${declared.case}`;
    const cell = cases.get(subject);
    if (!cell) {
      ctx.add("capability-cell-missing", subject, `no cell for the declared ${declared.kind} case`);
    } else if (cell.typedResult !== declared.kind) {
      ctx.add(
        "capability-cell-mismatch",
        subject,
        `declared ${declared.kind}, got ${cell.typedResult}`,
      );
    } else if (cell.limitation === undefined) {
      ctx.add("capability-limitation-missing", subject, "the cell states no limitation");
    }
  }
}

/** One row per surface the catalog declares, each truthful about its class. */
export function checkCapabilities(ctx) {
  const surfaces = ctx.graph.closure.capabilitySurfaces;
  const declared = new Set(surfaces.map((surface) => surface.surface));
  const rows = ctx.index(ctx.report.capabilities, (row) => row.surface, "capability-row-duplicate");
  for (const surface of surfaces) {
    const row = rows.get(surface.surface);
    const subject = surface.surface;
    if (!row) {
      ctx.add("capability-row-missing", subject, `no row for the ${surface.class} surface`);
      continue;
    }
    if (row.stabilityClass !== surface.class) {
      ctx.add(
        "capability-class-mismatch",
        subject,
        `declared ${surface.class}, got ${row.stabilityClass}`,
      );
    }
    for (const field of ["typedResult", "consumer", "executedVerification"]) {
      if (row[field] === undefined) ctx.add("capability-field-missing", subject, `no ${field}`);
    }
    if (row.executedVerification !== undefined && !ctx.evidenceById.has(row.executedVerification)) {
      ctx.add(
        "evidence-ref-dangling",
        subject,
        `evidence row ${row.executedVerification} does not exist`,
      );
    }
    if (
      surface.class === "stable" &&
      row.typedResult !== undefined &&
      row.typedResult !== "complete"
    ) {
      ctx.add("capability-noncomplete-stands-in", subject, "a stable surface must be complete");
    }
    if (surface.class !== "stable" && row.typedResult === "complete") {
      ctx.add(
        "capability-preview-claims-complete",
        subject,
        `a ${surface.class} surface is partial`,
      );
    } else if (surface.class !== "stable" && row.typedResult === "unsupported") {
      ctx.add(
        "capability-preview-not-partial",
        subject,
        `a ${surface.class} surface reports a truthful partial result, not unsupported`,
      );
    }
    if (
      row.typedResult !== "complete" &&
      row.typedResult !== undefined &&
      row.limitation === undefined
    ) {
      ctx.add("capability-limitation-missing", subject, "a partial surface states no limitation");
    }
    checkCapabilityCases(ctx, surface, row);
  }
  for (const surface of rows.keys()) {
    if (!declared.has(surface))
      ctx.add("capability-surface-unknown", surface, "not a declared surface");
  }
}

const MEASURED = new Set(["pass", "fail"]);

/**
 * One controlling cell. A cell is unavailable when its result or any mode
 * outcome is `unavailable`: it is explicitly unmeasured, so it needs a reason,
 * never counts as a measured pass, and is only allowed for a performance
 * observation (correctness, lifecycle and accounting evidence is never
 * optional). A failing result or outcome rejects whatever else the row says,
 * and anything that was measured must come from a benchmark worker.
 */
function checkMetricRow(ctx, cell, row) {
  const { policy } = ctx;
  const subject = cell.row;
  if (row.evidenceRun !== cell.evidenceRun) {
    ctx.add(
      "metric-run-mismatch",
      subject,
      `declared under ${cell.evidenceRun}, got ${row.evidenceRun}`,
    );
  }
  if (row.result === undefined) {
    ctx.add("metric-result-missing", subject, "the cell has no result");
  } else if (row.result === "fail") {
    ctx.add("metric-failing", subject, "failing cell returns to its owner");
  }
  let unavailable = row.result === "unavailable";
  const values = [row.result];
  for (const mode of cell.modes) {
    const outcome = row.outcomes?.[mode];
    values.push(outcome);
    if (outcome === undefined) {
      if (row.result !== "unavailable") {
        ctx.add("metric-outcome-missing", subject, `no ${mode} outcome`);
      }
    } else if (outcome === "fail") {
      ctx.add("metric-failing", subject, `the ${mode} outcome failed`);
    } else if (outcome === "unavailable") {
      unavailable = true;
    }
  }
  if (unavailable) {
    if (row.result === "pass") {
      ctx.add("metric-unmeasured-pass", subject, "an unmeasured mode cannot make a measured pass");
    }
    if (cell.evidenceKind !== "performance") {
      ctx.add(
        "metric-required-evidence-unavailable",
        subject,
        `${cell.evidenceKind} evidence is required, never an optional performance observation`,
      );
    }
    if (row.unavailableReason === undefined) {
      ctx.add("metric-result-missing", subject, "an unavailable cell states no reason");
    } else {
      ctx.advisories.push({ code: "metric-unavailable", subject, message: row.unavailableReason });
    }
  }
  const measured = values.some((value) => MEASURED.has(value));
  if (measured && !row.worker?.tags.includes(policy.metricWorkerTag)) {
    ctx.add(
      "metric-worker-not-allowed",
      subject,
      `not measured on a ${policy.metricWorkerTag} worker`,
    );
  }
  const required = measured || cell.evidenceKind !== "performance";
  if (
    required &&
    !ctx.report.evidence.some((evidence) => evidence.evidenceRun === cell.evidenceRun)
  ) {
    ctx.add("metric-run-evidence-missing", subject, `no evidence row for run ${cell.evidenceRun}`);
  }
}

/** The controlling cells declared by the export, each from a named benchmark run. */
export function checkMetrics(ctx) {
  const cells = ctx.graph.closure.metricCells;
  const populated = new Set(cells.map((cell) => cell.evidenceRun));
  for (const run of ctx.policy.metricEvidenceRuns) {
    if (!populated.has(run)) {
      ctx.add(
        "plan-field-missing",
        `closure.metricCells/${run}`,
        "the export declares no cell for this run",
      );
    }
  }
  const rows = ctx.index(ctx.report.metrics, (row) => row.row, "metric-row-duplicate");
  for (const cell of cells) {
    const row = rows.get(cell.row);
    if (!row) ctx.add("metric-cell-missing", cell.row, "a required cell has no row");
    else checkMetricRow(ctx, cell, row);
  }
  const declared = new Set(cells.map((cell) => cell.row));
  for (const row of rows.keys()) {
    if (!declared.has(row)) ctx.add("metric-undeclared", row, "not a declared controlling cell");
  }
}

function checkEvidenceRow(ctx, evidence, knownOwner) {
  const subject = evidence.id;
  if (evidence.owner === undefined) ctx.add("evidence-owner-missing", subject, "no owning task");
  else if (!knownOwner(evidence.owner)) {
    ctx.add("evidence-owner-unknown", subject, `${evidence.owner} is not a required owner`);
  }
  if (evidence.command === undefined && evidence.evidenceRun === undefined) {
    ctx.add("evidence-command-missing", subject, "names neither a command nor an evidence run");
  }
  if (evidence.candidate !== ctx.report.candidate) {
    ctx.add("evidence-stale-candidate", subject, "does not cover this candidate");
  }
  if (evidence.executor === undefined) {
    ctx.add("evidence-executor-missing", subject, "no local-execution or CI owner");
  }
  if (evidence.prerequisites === undefined) {
    ctx.add("evidence-prerequisites-missing", subject, "states no prerequisites");
  } else if (evidence.prerequisites.some((prerequisite) => !prerequisite.matched)) {
    ctx.add(
      "evidence-prerequisite-mismatch",
      subject,
      "a prerequisite did not match the candidate",
    );
  }
  const summary = evidence.terminalSummary;
  if (summary?.complete !== true || summary.passed === undefined) {
    ctx.add("evidence-summary-incomplete", subject, "no complete terminal summary");
    return;
  }
  if (!summary.passed) ctx.add("evidence-failed", subject, "the terminal summary did not pass");
  if (!(summary.selected > 0) || !(summary.executed > 0)) {
    ctx.add("evidence-zero-work", subject, "selected or executed no work");
  } else if (summary.executed + (summary.expectedSkips ?? 0) !== summary.selected) {
    ctx.add(
      "evidence-partial-execution",
      subject,
      `executed ${summary.executed} and expected to skip ${summary.expectedSkips ?? 0} of ${summary.selected} selected`,
    );
  }
  if (summary.unexpectedSkips !== 0) {
    ctx.add("evidence-unexpected-skip", subject, "expected exactly zero unexpected skips");
  }
}

/** Every evidence row proves execution; the required lanes and named rows must all be present. */
export function checkEvidence(ctx) {
  const { graph, policy, report, target } = ctx;
  const ancestors = graph.ancestorsOf(target);
  const knownOwner = (slug) => slug === target || ancestors.has(slug);
  for (const evidence of report.evidence) checkEvidenceRow(ctx, evidence, knownOwner);
  const lanes = new Set(report.evidence.map((evidence) => evidence.lane));
  for (const lane of policy.requiredEvidenceLanes) {
    if (!lanes.has(lane)) ctx.add("evidence-lane-missing", lane, "a required lane has no evidence");
  }
  const owners = new Set(report.evidence.map((evidence) => evidence.owner));
  for (const { owner, code } of policy.namedEvidence) {
    if (!owners.has(owner)) ctx.add(code, owner, "the named evidence row is absent");
  }
}

/** No undispositioned exception, open deferral or unexplained mismatch survives closure. */
export function checkExceptions(ctx) {
  for (const exception of ctx.report.exceptions) {
    const subject = exception.id;
    if (exception.disposition === undefined || exception.disposition === "open") {
      ctx.add("exception-undispositioned", subject, `${exception.subject} has no disposition`);
    } else if (exception.kind === "deferral" && exception.disposition === "accepted") {
      ctx.add("open-deferral", subject, `${exception.subject} is still deferred`);
    }
    if (exception.disposition !== undefined && exception.rationale === undefined) {
      ctx.add("exception-unexplained", subject, `${exception.subject} states no rationale`);
    }
  }
}
