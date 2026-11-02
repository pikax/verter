// Owner population rules. The required population comes from the export's
// dependency graph; a report row can never add to it or vouch for a status,
// because the ledger is the only implementation authority.

const indexRows = (ctx) => ctx.index(ctx.report.owners, (row) => row.slug, "owner-row-duplicate");

function checkStatusClaims(ctx, rows) {
  for (const [slug, row] of rows) {
    if (row.status === undefined || !ctx.graph.has(slug)) continue;
    const trusted = ctx.graph.status(slug) ?? "absent";
    if (row.status !== trusted) {
      ctx.add(
        "owner-status-mismatch",
        slug,
        `the report claims ${row.status} but the ledger says ${trusted}`,
      );
    }
  }
}

function checkClaimedPath(ctx, slug, row, target) {
  if (row.dependencyPath && !ctx.graph.isRealPath(row.dependencyPath, slug, target)) {
    ctx.add("owner-path-invalid", slug, `claimed path to ${target} is not made of real edges`);
  }
}

function requireImplemented(ctx, slug, pendingCode) {
  const status = ctx.graph.status(slug);
  if (status === undefined) {
    ctx.add("required-owner-ledger-absent", slug, "required owner has no ledger row");
  } else if (status !== ctx.policy.implementedStatus) {
    ctx.add(pendingCode, slug, `required owner is ${status} in the ledger`);
  }
}

function checkRoutes(ctx, slug, row, inPopulation) {
  if (row.displacedRoutes === undefined) {
    ctx.add("owner-route-disposition-missing", slug, "no displaced-route disposition");
    return;
  }
  for (const route of row.displacedRoutes) {
    if (route.disposition !== "surviving") continue;
    if (route.survivingOwner === undefined || !inPopulation(route.survivingOwner)) {
      ctx.add("surviving-unowned-route", slug, `route ${route.route} survives with no known owner`);
    }
  }
}

function checkEvidenceRefs(ctx, slug, row, namedOwners) {
  const refs = row.evidenceRefs ?? [];
  for (const ref of refs) {
    if (!ctx.evidenceById.has(ref)) {
      ctx.add("evidence-ref-dangling", slug, `evidence row ${ref} does not exist`);
    }
  }
  if (namedOwners.has(slug)) return;
  const backed =
    refs.some((ref) => ctx.evidenceById.has(ref)) ||
    ctx.report.evidence.some((evidence) => evidence.owner === slug);
  if (!backed) ctx.add("owner-evidence-missing", slug, "no evidence row backs this owner");
}

function checkFinalOwner(ctx, slug, row, inPopulation) {
  if (row.soleFinalOwner === undefined) {
    ctx.add("owner-final-owner-missing", slug, "no sole final owner");
  } else if (!ctx.graph.has(row.soleFinalOwner)) {
    ctx.add("owner-unknown", slug, `final owner ${row.soleFinalOwner} is not in the export`);
  } else if (!inPopulation(row.soleFinalOwner)) {
    ctx.add("owner-pathless", slug, `final owner ${row.soleFinalOwner} has no path to the target`);
  }
}

function checkRequiredRow(ctx, slug, row, inPopulation, namedOwners) {
  if (row.outcome === undefined) ctx.add("owner-outcome-missing", slug, "no outcome");
  checkFinalOwner(ctx, slug, row, inPopulation);
  if (ctx.graph.task(slug).semanticRole === ctx.policy.historicalRole) return;
  if (!row.consumers?.length) ctx.add("owner-consumer-missing", slug, "no consumer");
  checkRoutes(ctx, slug, row, inPopulation);
  checkEvidenceRefs(ctx, slug, row, namedOwners);
}

function expectedClassification(ctx, slug) {
  return ctx.graph.task(slug).semanticRole === ctx.policy.historicalRole
    ? "historical"
    : "required";
}

/**
 * Each required outcome has exactly one claimant, and that claimant is an
 * implemented owner with a path to the target. The target is judged before it
 * is implemented, so it may carry only the terminal outcomes the export
 * declares as its own (`closure.targetOutcomes`). An optional, pending or
 * detached row can never carry a required outcome.
 */
function checkOutcomePopulation(ctx, inPopulation) {
  const { graph, policy, target } = ctx;
  const targetOwned = new Set(graph.closure.targetOutcomes);
  for (const id of graph.closure.requiredOutcomes) {
    const claimants = ctx.report.owners.filter((row) => row.outcomeIds?.includes(id));
    if (claimants.length === 0) {
      ctx.add("outcome-unmapped", id, "no owner row carries this required outcome");
    } else if (claimants.length > 1) {
      const names = claimants.map((row) => row.slug).join(", ");
      ctx.add("outcome-multiple-owners", id, `claimed by ${names}; an outcome has one owner`);
    }
    for (const { slug } of claimants) {
      if (slug === target) {
        if (!targetOwned.has(id)) {
          ctx.add(
            "outcome-owner-ineligible",
            id,
            `${target} may own only its declared terminal outcomes`,
          );
        }
      } else if (!inPopulation(slug)) {
        ctx.add("outcome-owner-ineligible", id, `${slug} has no dependency path to ${target}`);
      } else if (graph.status(slug) !== policy.implementedStatus) {
        ctx.add("outcome-owner-ineligible", id, `${slug} is not implemented`);
      }
    }
  }
}

/** Whether the export marks `slug` as an optional task that was not selected. */
const isUnselectedOptional = (graph, slug) =>
  graph.task(slug)?.optional === true && graph.task(slug).selected === false;

/** The owner rules of the full closure population. */
export function checkClosureOwners(ctx) {
  const { graph, policy, target } = ctx;
  const ancestors = graph.ancestorsOf(target);
  const inPopulation = (slug) => slug === target || ancestors.has(slug);
  const namedEvidenceOwners = new Set(policy.namedEvidence.map((row) => row.owner));
  const rows = indexRows(ctx);

  for (const slug of graph.closure.namedOwners) {
    if (!graph.has(slug)) ctx.add("owner-unknown", slug, "a named owner is not in the export");
    else if (!inPopulation(slug))
      ctx.add("owner-pathless", slug, "no dependency path to the target");
  }
  for (const slug of policy.conditionalOwners) {
    const task = graph.task(slug);
    if (!task) ctx.add("owner-unknown", slug, "the conditional owner is not in the export");
    else if ((!task.optional || task.selected) && !ancestors.has(slug)) {
      ctx.add("conditional-owner-pathless", slug, "selected but has no path to the target");
    }
  }

  for (const slug of ancestors) {
    requireImplemented(ctx, slug, "ancestor-pending");
    const row = rows.get(slug);
    if (!row) {
      ctx.add("owner-row-missing", slug, "a required owner has no report row");
      continue;
    }
    if (row.classification !== expectedClassification(ctx, slug)) {
      ctx.add(
        "owner-classification-mismatch",
        slug,
        `expected ${expectedClassification(ctx, slug)}, got ${row.classification ?? "none"}`,
      );
    }
    checkClaimedPath(ctx, slug, row, target);
    checkRequiredRow(ctx, slug, row, inPopulation, namedEvidenceOwners);
  }

  const trains = new Set([...ancestors].map((slug) => graph.task(slug).train));
  for (const task of graph.tasks.values()) {
    if (!task.optional || task.selected || ancestors.has(task.slug)) continue;
    if (!trains.has(task.train)) continue;
    const row = rows.get(task.slug);
    if (!row) {
      ctx.add("optional-row-unclassified", task.slug, "an unselected optional owner has no row");
    } else if (row.classification !== "optional") {
      ctx.add("owner-classification-mismatch", task.slug, "unselected owner must be optional");
    }
  }

  for (const [slug, row] of rows) {
    if (slug === target) {
      checkRequiredRow(ctx, slug, row, inPopulation, namedEvidenceOwners);
    } else if (!graph.has(slug)) {
      ctx.add("owner-unknown", slug, "a report row names an owner outside the export");
    } else if (ancestors.has(slug)) {
      continue;
    } else if (row.classification !== "optional") {
      ctx.add("owner-pathless", slug, "a report row names an owner with no path to the target");
    } else if (!isUnselectedOptional(graph, slug) && !policy.conditionalOwners.includes(slug)) {
      // A selected conditional owner without a path is conditional-owner-pathless above.
      ctx.add("owner-pathless", slug, "only an unselected optional task may be classed optional");
    }
  }

  checkStatusClaims(ctx, rows);
  checkOutcomePopulation(ctx, inPopulation);
}

/**
 * Structural owners must be implemented; everything later is listed as pending,
 * never failed. The structural population is the export's list of owners the
 * lock names, never derived from the lock's own edges: an owner whose edge to
 * the lock was dropped must surface as pathless, not vanish into a later owner.
 */
export function checkOwnerLockOwners(ctx) {
  const { graph, policy, target } = ctx;
  const structural = new Set(graph.closure.structuralOwners);
  const lockAncestors = graph.ancestorsOf(target);
  const rows = indexRows(ctx);

  for (const slug of lockAncestors) {
    requireImplemented(
      ctx,
      slug,
      structural.has(slug) ? "structural-owner-pending" : "ancestor-pending",
    );
  }
  for (const slug of structural) {
    if (!graph.has(slug)) {
      ctx.add("owner-unknown", slug, "a structural owner is not in the export");
      continue;
    }
    if (!lockAncestors.has(slug)) {
      ctx.add("owner-pathless", slug, `a structural owner has no dependency path to ${target}`);
      requireImplemented(ctx, slug, "structural-owner-pending");
    }
    const row = rows.get(slug);
    if (!row) {
      ctx.add("structural-owner-row-missing", slug, "a structural owner has no report row");
    } else if (row.dependencyPath === undefined) {
      ctx.add("owner-path-missing", slug, `no dependency path to ${target}`);
    } else {
      checkClaimedPath(ctx, slug, row, target);
    }
  }

  const later = graph.has(policy.closureTarget)
    ? graph.ancestorsOf(policy.closureTarget)
    : new Set();
  for (const slug of later) {
    if (slug === target || lockAncestors.has(slug)) continue;
    const status = graph.status(slug) ?? "absent";
    if (!rows.has(slug)) {
      ctx.add("nonstructural-owner-unlisted", slug, "the report does not list this later owner");
    }
    if (status !== policy.implementedStatus) {
      ctx.pending.push({ slug, train: graph.task(slug).train, status });
    }
  }

  for (const slug of rows.keys()) {
    if (slug === target || lockAncestors.has(slug) || later.has(slug)) continue;
    if (slug === policy.closureTarget) continue;
    const code = graph.has(slug) ? "owner-pathless" : "owner-unknown";
    ctx.add(code, slug, "a report row names an owner outside the lock's population");
  }
  checkStatusClaims(ctx, rows);
}
