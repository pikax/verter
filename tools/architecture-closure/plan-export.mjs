// Reads the supplied plan export: the dependency graph and the trusted ledger.
// A population the rules need and the export lacks is `plan-field-missing`;
// nothing here falls back to prose, the repository or a live service.

const isObject = (v) => v !== null && typeof v === "object" && !Array.isArray(v);
const isText = (v) => typeof v === "string" && v.length > 0;
const isTextList = (v) => Array.isArray(v) && v.every(isText);
const oneOf = (values) => (v) => values.includes(v);

const STABILITY_CLASSES = ["stable", "preview", "experimental-unranked"];
const CASE_KINDS = ["partial", "unsupported"];
const EVIDENCE_KINDS = ["performance", "correctness", "lifecycle", "accounting"];
const GUARD_KINDS = ["dependency", "feature", "capability", "owner"];
const ADMISSION_CLASSES = [
  "CompleteFact",
  "CertifiedDivergence",
  "OperationRefusal",
  "IsolatedRootLedgerRefusal",
  "Never",
];

/** Field name to predicate, for one record element of a population. */
const record = (fields) => (element) =>
  isObject(element)
    ? Object.entries(fields)
        .filter(([field, valid]) => !valid(element[field]))
        .map(([field]) => `.${field}`)
    : [""];
const text = (element) => (isText(element) ? [] : [""]);
const caseList = (cases) =>
  Array.isArray(cases) &&
  cases.every((entry) => record({ case: isText, kind: oneOf(CASE_KINDS) })(entry).length === 0);

/**
 * Every population the rules need, with its element shape and the key that
 * makes an element unique. Each must be present, non-empty and well formed:
 * an empty or partial population would let its rule pass vacuously.
 */
const POPULATIONS = {
  structuralOwners: { element: text, key: (v) => v },
  namedOwners: { element: text, key: (v) => v },
  requiredOutcomes: { element: text, key: (v) => v },
  capabilitySurfaces: {
    element: record({ surface: isText, class: oneOf(STABILITY_CLASSES), cases: caseList }),
    key: (v) => v.surface,
  },
  metricCells: {
    element: record({
      row: isText,
      evidenceRun: isText,
      modes: (v) => isTextList(v) && v.length > 0,
      evidenceKind: oneOf(EVIDENCE_KINDS),
    }),
    key: (v) => v.row,
  },
  wsp6Counts: { element: text, key: (v) => v },
  engineRelocations: {
    element: record({ from: isText, to: isText }),
    key: (v) => `${v.from} -> ${v.to}`,
  },
  engineGuardFamilies: {
    element: record({ family: isText, kind: oneOf(GUARD_KINDS) }),
    key: (v) => v.family,
  },
  futureCharterSurfaces: {
    element: record({ surface: isText, owner: isText }),
    key: (v) => v.surface,
  },
  observeOwners: { element: record({ owner: isText, path: isText }), key: (v) => v.owner },
  reuseAdmissions: {
    element: record({ id: isText, class: oneOf(ADMISSION_CLASSES) }),
    key: (v) => v.id,
  },
};

const STRUCTURAL_INVENTORIES = [
  "wsp6Counts",
  "engineRelocations",
  "engineGuardFamilies",
  "futureCharterSurfaces",
  "observeOwners",
];
const POPULATION_FIELDS = {
  "owner-lock": ["structuralOwners", ...STRUCTURAL_INVENTORIES],
  closure: [
    "namedOwners",
    "requiredOutcomes",
    "capabilitySurfaces",
    "metricCells",
    "reuseAdmissions",
    ...STRUCTURAL_INVENTORIES,
  ],
};

function populationProblems(closure, field) {
  const at = `closure.${field}`;
  const list = closure[field];
  const missing = "plan-field-missing";
  if (!Array.isArray(list)) return [[missing, at, `the export has no ${at} population`]];
  if (list.length === 0) return [[missing, at, `the export's ${at} population is empty`]];
  const { element, key } = POPULATIONS[field];
  const out = [];
  const seen = new Set();
  list.forEach((entry, index) => {
    const bad = element(entry);
    for (const suffix of bad) {
      const subject = `${at}[${index}]${suffix}`;
      out.push([missing, subject, `a ${at} element lacks a valid ${suffix || "value"}`]);
    }
    if (bad.length > 0) return;
    const identity = key(entry);
    if (seen.has(identity)) {
      out.push(["plan-duplicate-entry", `${at}[${index}]`, `${identity} is listed twice`]);
    }
    seen.add(identity);
  });
  return out;
}

function taskFieldProblems(task, index) {
  const at = `tasks[${index}]`;
  if (!isObject(task)) return [at];
  const out = [];
  if (!isText(task.slug)) out.push(`${at}.slug`);
  if (!isText(task.train)) out.push(`${at}.train`);
  if (!isTextList(task.predecessors)) out.push(`${at}.predecessors`);
  if (!isTextList(task.labels)) out.push(`${at}.labels`);
  if (typeof task.optional !== "boolean") out.push(`${at}.optional`);
  if (!isText(task.semanticRole)) out.push(`${at}.semanticRole`);
  if (task.optional === true && typeof task.selected !== "boolean") out.push(`${at}.selected`);
  return out;
}

class PlanGraph {
  constructor(plan) {
    this.tasks = new Map(plan.tasks.map((task) => [task.slug, task]));
    this.ledger = new Map(plan.ledger.map((row) => [row.slug, row.status]));
    this.closure = plan.closure;
    this.ancestorCache = new Map();
  }

  has(slug) {
    return this.tasks.has(slug);
  }

  task(slug) {
    return this.tasks.get(slug);
  }

  status(slug) {
    return this.ledger.get(slug);
  }

  /** Every transitive predecessor of `slug`; a cycle terminates on the visited set. */
  ancestorsOf(slug) {
    const cached = this.ancestorCache.get(slug);
    if (cached) return cached;
    const seen = new Set();
    const stack = [slug];
    while (stack.length > 0) {
      for (const predecessor of this.tasks.get(stack.pop())?.predecessors ?? []) {
        if (!seen.has(predecessor)) {
          seen.add(predecessor);
          stack.push(predecessor);
        }
      }
    }
    this.ancestorCache.set(slug, seen);
    return seen;
  }

  /**
   * Edges reachable from `root` that leave the export. A task elsewhere in the
   * plan may depend on another plan's task; that never affects this population.
   */
  danglingFrom(root) {
    const out = [];
    for (const slug of [root, ...this.ancestorsOf(root)]) {
      for (const predecessor of this.tasks.get(slug)?.predecessors ?? []) {
        if (!this.tasks.has(predecessor)) out.push({ slug, predecessor });
      }
    }
    return out;
  }

  /** A path naming consecutive real edges from `owner` up to `target`. */
  isRealPath(path, owner, target) {
    if (path.length < 2 || path[0] !== owner || path.at(-1) !== target) return false;
    return path.every(
      (slug, index) =>
        index === 0 || this.tasks.get(slug)?.predecessors.includes(path[index - 1]) === true,
    );
  }
}

/**
 * Validates the export's shape for `mode`. Returns the graph, or the problems
 * that stop the population from being derived at all.
 */
export function readPlanExport(plan, mode) {
  const problems = [];
  const missing = (subject, message) =>
    problems.push({ code: "plan-field-missing", subject, message });
  if (!isObject(plan)) {
    missing("export", "the plan export is not an object");
    return { problems };
  }
  if (!Array.isArray(plan.tasks)) missing("tasks", "the export has no tasks[]");
  if (!Array.isArray(plan.ledger)) missing("ledger", "the export has no ledger[] status rows");
  if (!isObject(plan.closure)) missing("closure", "the export has no closure populations");
  else {
    for (const field of POPULATION_FIELDS[mode]) {
      for (const [code, subject, message] of populationProblems(plan.closure, field)) {
        problems.push({ code, subject, message });
      }
    }
  }
  if (Array.isArray(plan.tasks)) {
    plan.tasks.forEach((task, index) => {
      for (const subject of taskFieldProblems(task, index)) {
        missing(subject, `the export's task record lacks ${subject}`);
      }
    });
  }
  if (Array.isArray(plan.ledger)) {
    plan.ledger.forEach((row, index) => {
      if (!isObject(row) || !isText(row.slug) || !isText(row.status)) {
        missing(`ledger[${index}]`, "a ledger row needs slug and status");
      }
    });
  }
  if (problems.length > 0) return { problems };

  const seenSlugs = new Set();
  for (const task of plan.tasks) {
    if (seenSlugs.has(task.slug)) {
      problems.push({
        code: "plan-duplicate-task",
        subject: task.slug,
        message: "the export lists this task twice",
      });
    }
    seenSlugs.add(task.slug);
  }
  const seenLedger = new Set();
  for (const row of plan.ledger) {
    if (seenLedger.has(row.slug)) {
      problems.push({
        code: "plan-duplicate-ledger-row",
        subject: row.slug,
        message: "two ledger rows make this task's status ambiguous",
      });
    }
    seenLedger.add(row.slug);
  }
  if (problems.length > 0) return { problems };
  return { problems, graph: new PlanGraph(plan) };
}
