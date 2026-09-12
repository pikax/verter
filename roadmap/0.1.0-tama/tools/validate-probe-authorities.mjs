#!/usr/bin/env node
// Validation-probe authority validator.
//
// Every `{ authority, atom }` citation in an executable probe artifact — the
// probe-state manifests (`crates/verter_validation_probe/manifest/*.toml`),
// the promotion join records (`crates/verter_validation_probe/join/*.toml`),
// and observation artifacts (`observations.json`) — is checked against the
// validation-authority catalog, the owning node's charter text, and the
// trusted implementation ledger:
//
//   * the authority id is catalogued, its row covers the cited framework and
//     dimension, and the atom is a durable id listed for that authority whose
//     own dimensions cover the cell (a roadmap-shaped atom is refused);
//   * a gate, a join promotion, and a `comparison_eligible = true` basis
//     require the owning node to be implemented; a gate's exact expected
//     class must be listed in the cited atom's outcomes;
//   * a manifest declaring `comparison = structural` names exactly the
//     comparator its framework's product authority supplies, and every cell
//     of a dimension the manifest declares inapplicable is a skip owned by
//     that product authority.
//
// The catalog itself is checked first: every atom's `charter_atom` must
// resolve in the owning node's charter.
//
// Usage:
//   node validate-probe-authorities.mjs [--manifest DIR] [--join DIR]
//     [--observations FILE|DIR] [--catalog FILE] [--ledger FILE]
//
// Relative paths resolve against the working directory; defaults resolve
// against the repository root. The manifest directory must exist. A join
// directory that does not exist holds no records. An observations directory
// is walked for `*/observations.json`, each file validated separately.

import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { PACKAGE_ROOT, loadAuthority } from "./lib.mjs";
import { parseLedgerText, STATUS_IMPLEMENTED } from "./ledger.mjs";
import { parseToml } from "./toml.mjs";

export const REPO_ROOT = path.resolve(PACKAGE_ROOT, "..", "..");
export const DEFAULT_CATALOG = path.join(PACKAGE_ROOT, "catalogs", "validation-authorities.toml");
export const DEFAULT_MANIFEST_DIR = path.join(
  REPO_ROOT,
  "crates",
  "verter_validation_probe",
  "manifest",
);
export const DEFAULT_JOIN_DIR = path.join(REPO_ROOT, "crates", "verter_validation_probe", "join");

export const DIMENSIONS = ["Route", "Compile", "Structural", "Runtime", "Map", "Performance"];
export const FRAMEWORKS = ["vue", "svelte"];
const CATALOG_FRAMEWORKS = [...FRAMEWORKS, "any"];
const EXPECTED_STATES = ["gate", "canary", "known-fail", "skip"];
const JOIN_TRANSITIONS = [
  "skip -> canary",
  "canary -> known-fail",
  "known-fail -> gate",
  "canary -> gate",
  "deferred",
];
const DISPOSITIONS = ["deferred", "adopted", "discarded"];
const EQUIVALENT_WORK_AUTHORITY = "compiler.equivalent-work-ledger";

const ROUTE_CLASSES = [
  "pass",
  "harness_failure",
  "crash",
  "timeout",
  "host_failure",
  "request_refused",
];
const COMPILE_CLASSES = [
  ...ROUTE_CLASSES,
  "unsupported",
  "verter_diagnostic",
  "product_not_produced",
  "product_malformed",
];
/** The closed class/dimension admissibility matrix. */
export const ADMISSIBLE = {
  Route: ROUTE_CLASSES,
  Compile: COMPILE_CLASSES,
  Structural: [...COMPILE_CLASSES, "reference_failure", "semantic_mismatch"],
  Runtime: [...COMPILE_CLASSES, "runtime_mismatch"],
  Map: [...COMPILE_CLASSES, "source_map_mismatch"],
  Performance: ["pass", "harness_failure", "crash", "timeout"],
};
const INAPPLICABLE_REASON = {
  Structural: "comparison = none",
  Runtime: "applicability.runtime = inapplicable",
  Map: "applicability.map = inapplicable",
};

const DURABLE_ID = /^[a-z][a-z0-9]*(?:-[a-z0-9]+)*$/u;
const AUTHORITY_ID = /^[a-z][a-z0-9]*(?:-[a-z0-9]+)*\.[a-z][a-z0-9]*(?:-[a-z0-9]+)*$/u;
const ROADMAP_SHAPED = /[A-Z]+[0-9]+[A-Z0-9]*(?:-AC[0-9]+)?/u;
const ACCEPTANCE_ID = /^[A-Z][A-Z0-9]*-AC[0-9]+$/u;
const SECTION_ATOM = /^(.+)\/([1-9][0-9]*)$/u;
const COMPARATOR_KEYS = ["crate", "path", "function", "atom"];

function isTable(value) {
  return value !== null && typeof value === "object" && !Array.isArray(value);
}

function unknownKeys(table, allowed, where) {
  return Object.keys(table)
    .filter((key) => !allowed.includes(key))
    .map((key) => `${where}: unknown field ${key}`);
}

function stringList(value) {
  return Array.isArray(value) && value.every((item) => typeof item === "string");
}

/**
 * Whether `charterAtom` names an acceptance atom that exists in the charter
 * text: an acceptance id (`**<id>` in the text), or "<section heading>/<n>"
 * (the n-th top-level bullet of the `## <section heading>` section).
 */
export function charterAtomExists(charterText, charterAtom) {
  if (typeof charterText !== "string" || typeof charterAtom !== "string") return false;
  if (ACCEPTANCE_ID.test(charterAtom))
    return charterText.split("\n").some((line) => line.startsWith(`- **${charterAtom} `));
  const section = charterAtom.match(SECTION_ATOM);
  if (!section) return false;
  const [, heading, ordinal] = section;
  const lines = charterText.replaceAll("\r\n", "\n").split("\n");
  const start = lines.indexOf(`## ${heading}`);
  if (start < 0) return false;
  let bullets = 0;
  for (const line of lines.slice(start + 1)) {
    if (line.startsWith("## ")) break;
    if (line.startsWith("- ")) bullets += 1;
  }
  return Number(ordinal) <= bullets;
}

/**
 * Validate the catalog document. `charterFor(nodeId)` returns the owning
 * charter's text, or undefined for an unknown node.
 */
export function validateCatalog(catalog, charterFor) {
  const errors = [];
  const authorities = new Map();
  const where = "validation-authorities catalog";
  if (!isTable(catalog)) return { errors: [`${where}: document must be a table`], authorities };
  errors.push(...unknownKeys(catalog, ["schema", "authority"], where));
  if (catalog.schema !== 1) errors.push(`${where}: schema must be 1`);
  if (!Array.isArray(catalog.authority) || catalog.authority.length === 0) {
    errors.push(`${where}: missing [[authority]] rows`);
    return { errors, authorities };
  }
  const productAuthority = new Map();
  for (const row of catalog.authority) {
    const at = `${where}: authority ${row?.id ?? "<missing id>"}`;
    if (!isTable(row)) {
      errors.push(`${at}: row must be a table`);
      continue;
    }
    errors.push(
      ...unknownKeys(row, ["id", "node", "framework", "dimensions", "comparator", "atoms"], at),
    );
    if (typeof row.id !== "string" || !AUTHORITY_ID.test(row.id)) {
      errors.push(`${at}: id must be a durable dotted lower-kebab id`);
      continue;
    }
    if (authorities.has(row.id)) {
      errors.push(`${at}: duplicate authority id`);
      continue;
    }
    const charterText = typeof row.node === "string" ? charterFor(row.node) : undefined;
    if (charterText === undefined)
      errors.push(`${at}: node ${row.node} is not a DAG node with a charter`);
    if (!CATALOG_FRAMEWORKS.includes(row.framework))
      errors.push(`${at}: framework must be one of ${CATALOG_FRAMEWORKS.join(", ")}`);
    if (FRAMEWORKS.includes(row.framework)) {
      if (productAuthority.has(row.framework))
        errors.push(
          `${at}: framework ${row.framework} already has product authority ${productAuthority.get(row.framework)}`,
        );
      else productAuthority.set(row.framework, row.id);
    }
    const dimensions = stringList(row.dimensions) ? row.dimensions : [];
    if (dimensions.length === 0 || dimensions.some((dimension) => !DIMENSIONS.includes(dimension)))
      errors.push(`${at}: dimensions must be a non-empty list of ${DIMENSIONS.join(", ")}`);
    const atoms = new Map();
    if (!isTable(row.atoms) || Object.keys(row.atoms).length === 0) {
      errors.push(`${at}: atoms must be a non-empty table keyed by durable atom id`);
    } else {
      for (const [atomId, atom] of Object.entries(row.atoms)) {
        const atomAt = `${at}: atom ${atomId}`;
        if (!DURABLE_ID.test(atomId)) errors.push(`${atomAt}: atom id must be durable lower-kebab`);
        if (!isTable(atom)) {
          errors.push(`${atomAt}: atom must be a table`);
          continue;
        }
        errors.push(...unknownKeys(atom, ["charter_atom", "dimensions", "outcomes"], atomAt));
        if (charterText !== undefined && !charterAtomExists(charterText, atom.charter_atom))
          errors.push(
            `${atomAt}: charter_atom ${JSON.stringify(atom.charter_atom)} does not exist in ${row.node}'s charter`,
          );
        const atomDimensions = stringList(atom.dimensions) ? atom.dimensions : [];
        if (
          atomDimensions.length === 0 ||
          atomDimensions.some((dimension) => !dimensions.includes(dimension))
        )
          errors.push(
            `${atomAt}: dimensions must be a non-empty subset of the authority's dimensions`,
          );
        const outcomes = stringList(atom.outcomes) ? atom.outcomes : [];
        if (outcomes.length === 0) errors.push(`${atomAt}: outcomes must be a non-empty list`);
        for (const outcome of outcomes)
          for (const dimension of atomDimensions)
            if (!(ADMISSIBLE[dimension] || []).includes(outcome))
              errors.push(`${atomAt}: outcome ${outcome} is not admissible at ${dimension}`);
        atoms.set(atomId, { dimensions: atomDimensions, outcomes });
      }
    }
    let comparator = null;
    if (row.comparator !== undefined) {
      if (!FRAMEWORKS.includes(row.framework))
        errors.push(`${at}: only a framework product authority supplies a comparator`);
      if (
        !isTable(row.comparator) ||
        COMPARATOR_KEYS.some((key) => typeof row.comparator[key] !== "string")
      ) {
        errors.push(`${at}: comparator must be { ${COMPARATOR_KEYS.join(", ")} }`);
      } else {
        errors.push(...unknownKeys(row.comparator, COMPARATOR_KEYS, `${at}: comparator`));
        if (!atoms.get(row.comparator.atom)?.dimensions.includes("Structural"))
          errors.push(
            `${at}: comparator atom ${row.comparator.atom} is not an atom of this authority covering Structural`,
          );
        comparator = row.comparator;
      }
    }
    authorities.set(row.id, {
      id: row.id,
      node: row.node,
      framework: row.framework,
      dimensions,
      atoms,
      comparator,
    });
  }
  return { errors, authorities, productAuthority };
}

/**
 * Check one `{ authority, atom }` citation for a cell of `framework` at
 * `dimension`. `gateClass` (a gate's exact expected class) must be listed in
 * the atom's outcomes; `requireImplemented` requires an implemented owner.
 */
export function citationErrors(
  ctx,
  where,
  { framework, dimension, citation, requireImplemented, gateClass },
) {
  const authorityId = citation?.authority;
  const atomId = citation?.atom;
  if (typeof authorityId !== "string" || typeof atomId !== "string" || !authorityId || !atomId)
    return [`${where}: missing { authority, atom } citation`];
  if (ROADMAP_SHAPED.test(atomId) || atomId.includes("/"))
    return [`${where}: atom ${JSON.stringify(atomId)} is roadmap-shaped; cite the durable atom id`];
  if (!DURABLE_ID.test(atomId))
    return [`${where}: atom ${JSON.stringify(atomId)} is not a durable lower-kebab id`];
  const authority = ctx.authorities.get(authorityId);
  if (!authority) return [`${where}: unknown authority ${authorityId}`];
  const errors = [];
  if (authority.framework !== "any" && authority.framework !== framework)
    errors.push(
      `${where}: authority ${authorityId} covers framework ${authority.framework}, not ${framework}`,
    );
  if (!authority.dimensions.includes(dimension))
    errors.push(`${where}: authority ${authorityId} does not cover dimension ${dimension}`);
  const atom = authority.atoms.get(atomId);
  if (!atom) {
    errors.push(`${where}: atom ${atomId} is not listed for authority ${authorityId}`);
  } else {
    if (!atom.dimensions.includes(dimension))
      errors.push(
        `${where}: atom ${atomId} of ${authorityId} does not authorize dimension ${dimension}`,
      );
    if (gateClass !== undefined && !atom.outcomes.includes(gateClass))
      errors.push(
        `${where}: atom ${atomId} of ${authorityId} does not list gate class ${gateClass}`,
      );
  }
  if (requireImplemented && !ctx.implemented.has(authority.node))
    errors.push(
      `${where}: authority ${authorityId} is owned by ${authority.node}, whose ledger row is not implemented`,
    );
  return errors;
}

/** Validate one probe-state manifest's header and every cell citation. */
export function validateManifest(ctx, file, doc) {
  const errors = [];
  const where = `manifest ${file}`;
  if (!isTable(doc)) return [`${where}: document must be a table`];
  const framework = doc.framework;
  if (!FRAMEWORKS.includes(framework))
    return [`${where}: framework must be one of ${FRAMEWORKS.join(", ")}`];
  if (path.basename(file) !== `${framework}.toml`)
    errors.push(`${where}: file must be named ${framework}.toml`);
  const productId = ctx.productAuthority.get(framework);
  const product = productId ? ctx.authorities.get(productId) : undefined;
  if (!product)
    errors.push(`${where}: the catalog has no product authority for framework ${framework}`);

  if (doc.comparison === "structural") {
    if (product && !product.comparator)
      errors.push(
        `${where}: comparison = structural, but product authority ${productId} supplies no comparator`,
      );
    else if (!isTable(doc.comparator))
      errors.push(`${where}: comparison = structural requires a comparator`);
    else if (product) {
      const substituted = COMPARATOR_KEYS.filter(
        (key) => doc.comparator[key] !== product.comparator[key],
      );
      if (substituted.length || Object.keys(doc.comparator).length !== COMPARATOR_KEYS.length)
        errors.push(
          `${where}: comparator differs from ${productId}'s catalog comparator (${substituted.join(", ") || "extra fields"})`,
        );
    }
  } else if (doc.comparison === "none") {
    if (doc.comparator !== undefined)
      errors.push(`${where}: comparison = none forbids a comparator`);
  } else {
    errors.push(`${where}: comparison must be structural or none`);
  }

  const applicability = isTable(doc.applicability) ? doc.applicability : {};
  for (const key of ["runtime", "map"])
    if (!["applicable", "inapplicable"].includes(applicability[key]))
      errors.push(`${where}: applicability.${key} must be applicable or inapplicable`);
  const inapplicable = new Set();
  if (doc.comparison === "none") inapplicable.add("Structural");
  if (applicability.runtime === "inapplicable") inapplicable.add("Runtime");
  if (applicability.map === "inapplicable") inapplicable.add("Map");

  const entries = doc.entries === undefined ? [] : doc.entries;
  if (!Array.isArray(entries)) return [...errors, `${where}: entries must be an array of tables`];
  for (const [index, entry] of entries.entries()) {
    const cell = `${where}: ${entry?.probe_id ?? `entries[${index}]`} [${entry?.dimension ?? "?"}]`;
    if (!isTable(entry)) {
      errors.push(`${cell}: entry must be a table`);
      continue;
    }
    if (entry.framework !== framework)
      errors.push(`${cell}: framework ${entry.framework} differs from ${framework}`);
    if (!DIMENSIONS.includes(entry.dimension)) {
      errors.push(`${cell}: unknown dimension`);
      continue;
    }
    if (!EXPECTED_STATES.includes(entry.expected_state)) {
      errors.push(`${cell}: unknown expected_state ${entry.expected_state}`);
      continue;
    }
    const gate = entry.expected_state === "gate";
    if (gate && typeof entry.expected_class !== "string")
      errors.push(`${cell}: a gate requires an expected_class`);
    errors.push(
      ...citationErrors(ctx, cell, {
        framework,
        dimension: entry.dimension,
        citation: entry,
        requireImplemented: gate,
        gateClass:
          gate && typeof entry.expected_class === "string" ? entry.expected_class : undefined,
      }),
    );
    if (inapplicable.has(entry.dimension)) {
      if (entry.expected_state !== "skip" || entry.authority !== productId)
        errors.push(
          `${cell}: ${INAPPLICABLE_REASON[entry.dimension]}, so the cell must be a skip owned by ${productId}`,
        );
    }
  }
  return errors;
}

function frameworkOf(caseId) {
  if (typeof caseId !== "string") return undefined;
  const prefix = caseId.split("/")[0];
  return FRAMEWORKS.includes(prefix) ? prefix : undefined;
}

/** Validate one promotion join record's citations. */
export function validateJoin(ctx, file, doc) {
  const errors = [];
  const where = `join ${file}`;
  if (!isTable(doc)) return [`${where}: document must be a table`];
  for (const [key, rows] of [
    ["probe", doc.probe],
    ["observation", doc.observation],
  ])
    if (rows !== undefined && !Array.isArray(rows))
      errors.push(`${where}: ${key} must be an array of tables`);
  for (const [index, row] of (Array.isArray(doc.probe) ? doc.probe : []).entries()) {
    const at = `${where}: probe ${row?.probe_id ?? `[${index}]`} [${row?.dimension ?? "?"}]`;
    const framework = frameworkOf(row?.probe_id);
    if (!framework) {
      errors.push(`${at}: probe_id must be <framework>/<case>`);
      continue;
    }
    if (!DIMENSIONS.includes(row.dimension)) {
      errors.push(`${at}: unknown dimension`);
      continue;
    }
    if (!JOIN_TRANSITIONS.includes(row.transition)) {
      errors.push(`${at}: transition must be one of ${JOIN_TRANSITIONS.join(", ")}`);
      continue;
    }
    if (row.transition === "deferred") continue;
    errors.push(
      ...citationErrors(ctx, `${at}: ${row.transition}`, {
        framework,
        dimension: row.dimension,
        citation: row.decided_by,
        requireImplemented: true,
      }),
    );
  }
  for (const [index, row] of (Array.isArray(doc.observation) ? doc.observation : []).entries()) {
    const at = `${where}: observation ${row?.artifact_id ?? "?"} ${row?.row_id ?? `[${index}]`}`;
    if (!DISPOSITIONS.includes(row?.disposition)) {
      errors.push(`${at}: disposition must be one of ${DISPOSITIONS.join(", ")}`);
      continue;
    }
    if (row.disposition !== "adopted") continue;
    const framework = frameworkOf(row.row_id);
    if (!framework) {
      errors.push(`${at}: row_id must be <framework>/<case>@<mode>`);
      continue;
    }
    if (row.basis?.authority !== undefined && row.basis.authority !== EQUIVALENT_WORK_AUTHORITY)
      errors.push(`${at}: an adopted observation's basis must cite ${EQUIVALENT_WORK_AUTHORITY}`);
    errors.push(
      ...citationErrors(ctx, `${at}: adopted basis`, {
        framework,
        dimension: "Performance",
        citation: row.basis,
        requireImplemented: true,
      }),
    );
  }
  return errors;
}

/** Validate one observation artifact's comparison-eligibility citations. */
export function validateObservations(ctx, file, doc) {
  const errors = [];
  const where = `observations ${file}`;
  if (!isTable(doc) || !Array.isArray(doc.rows))
    return [`${where}: artifact must carry a rows array`];
  for (const [index, row] of doc.rows.entries()) {
    const at = `${where}: row ${row?.row_id ?? `[${index}]`}`;
    if (!isTable(row)) {
      errors.push(`${at}: row must be an object`);
      continue;
    }
    const framework = frameworkOf(row.case_id);
    if (!framework) {
      errors.push(`${at}: case_id must be <framework>/<case>`);
      continue;
    }
    if (row.comparison_eligible !== undefined && typeof row.comparison_eligible !== "boolean")
      errors.push(`${at}: comparison_eligible must be a boolean`);
    const eligible = row.comparison_eligible === true;
    const bases = [
      ["semantic_basis", "Structural"],
      ["equivalent_work_basis", "Performance"],
    ];
    for (const [key, dimension] of bases) {
      const basis = row[key];
      if (basis === undefined || basis === null) {
        if (eligible) errors.push(`${at}: comparison_eligible = true requires ${key}`);
        continue;
      }
      errors.push(
        ...citationErrors(ctx, `${at}: ${key}`, {
          framework,
          dimension,
          citation: basis,
          requireImplemented: eligible,
        }),
      );
      if (key === "equivalent_work_basis" && basis?.authority !== EQUIVALENT_WORK_AUTHORITY)
        errors.push(`${at}: ${key} must cite ${EQUIVALENT_WORK_AUTHORITY}`);
    }
    if (eligible && ctx.comparisonOf.get(framework) !== "structural")
      errors.push(
        `${at}: comparison_eligible = true, but no ${framework} manifest declares comparison = structural`,
      );
  }
  return errors;
}

/**
 * Validate the catalog and every supplied artifact. Pure over its inputs:
 *   catalog        parsed catalog document
 *   charterFor     nodeId -> charter text | undefined
 *   implemented    Set of implemented node ids
 *   manifests, joins, observations   [{ file, doc }]
 */
export function validateProbeAuthorities({
  catalog,
  charterFor,
  implemented,
  manifests = [],
  joins = [],
  observations = [],
}) {
  const { errors, authorities, productAuthority } = validateCatalog(catalog, charterFor);
  const ctx = {
    authorities,
    productAuthority: productAuthority || new Map(),
    implemented,
    comparisonOf: new Map(),
  };
  const seenFrameworks = new Set();
  for (const { file, doc } of manifests) {
    errors.push(...validateManifest(ctx, file, doc));
    if (isTable(doc) && FRAMEWORKS.includes(doc.framework)) {
      if (seenFrameworks.has(doc.framework))
        errors.push(`manifest ${file}: second manifest for framework ${doc.framework}`);
      seenFrameworks.add(doc.framework);
      ctx.comparisonOf.set(doc.framework, doc.comparison);
    }
  }
  for (const { file, doc } of joins) errors.push(...validateJoin(ctx, file, doc));
  for (const { file, doc } of observations) errors.push(...validateObservations(ctx, file, doc));
  return errors;
}

// Every `<subdirectory>/observations.json` under a directory, or `[target]`
// for a file.
export function observationFiles(target) {
  const stat = fs.statSync(target);
  if (stat.isFile()) return [target];
  return fs
    .readdirSync(target, { withFileTypes: true })
    .filter((entry) => entry.isDirectory())
    .map((entry) => path.join(target, entry.name, "observations.json"))
    .filter((file) => fs.existsSync(file) && fs.statSync(file).isFile())
    .sort();
}

function tomlFiles(dir, label) {
  const files = [];
  for (const entry of fs
    .readdirSync(dir, { withFileTypes: true })
    .sort((a, b) => (a.name < b.name ? -1 : 1))) {
    const file = path.join(dir, entry.name);
    if (entry.isFile() && entry.name.endsWith(".toml")) files.push(file);
    else if (!(entry.isFile() && entry.name.endsWith(".md")))
      throw new Error(
        `${label} ${dir}: unexpected entry ${entry.name} (only *.toml records and *.md notes)`,
      );
  }
  return files;
}

function readDocument(file, parse) {
  try {
    return { file, doc: parse(fs.readFileSync(file, "utf8")) };
  } catch (error) {
    throw new Error(`${file}: ${error.message}`);
  }
}

/** Load every input from disk. */
export function loadInputs({
  packageRoot = PACKAGE_ROOT,
  catalogFile = DEFAULT_CATALOG,
  ledgerFile,
  manifestDir = DEFAULT_MANIFEST_DIR,
  joinDir = DEFAULT_JOIN_DIR,
  observations,
} = {}) {
  const authority = loadAuthority(packageRoot);
  const charters = new Map(authority.nodes.map((node) => [node.id, node.charter]));
  const charterFor = (nodeId) => {
    const relative = charters.get(nodeId);
    if (typeof relative !== "string") return undefined;
    const file = path.join(packageRoot, relative);
    return fs.existsSync(file) ? fs.readFileSync(file, "utf8") : undefined;
  };
  const ledger = parseLedgerText(fs.readFileSync(ledgerFile ?? authority.ledgerFile, "utf8"));
  const implemented = new Set(
    Object.entries(ledger.implementation)
      .filter(([, record]) => record.status === STATUS_IMPLEMENTED)
      .map(([nodeId]) => nodeId),
  );
  if (!fs.existsSync(manifestDir) || !fs.statSync(manifestDir).isDirectory())
    throw new Error(`manifest directory ${manifestDir} does not exist`);
  const manifests = tomlFiles(manifestDir, "manifest directory").map((file) =>
    readDocument(file, parseToml),
  );
  const joins = fs.existsSync(joinDir)
    ? tomlFiles(joinDir, "join directory").map((file) => readDocument(file, parseToml))
    : [];
  let observationDocs = [];
  if (observations !== undefined) {
    if (!fs.existsSync(observations))
      throw new Error(`observations ${observations} does not exist`);
    observationDocs = observationFiles(observations).map((file) => readDocument(file, JSON.parse));
  }
  return {
    catalog: readDocument(catalogFile, parseToml).doc,
    charterFor,
    implemented,
    manifests,
    joins,
    observations: observationDocs,
  };
}

const FLAGS = {
  "--manifest": "manifestDir",
  "--join": "joinDir",
  "--observations": "observations",
  "--catalog": "catalogFile",
  "--ledger": "ledgerFile",
};

export function parseArgs(argv) {
  const options = {};
  for (let index = 0; index < argv.length; index += 1) {
    const key = FLAGS[argv[index]];
    const value = argv[index + 1];
    if (!key || value === undefined || value.startsWith("--"))
      throw new Error(
        `usage: validate-probe-authorities.mjs ${Object.keys(FLAGS)
          .map((flag) => `[${flag} PATH]`)
          .join(" ")}`,
      );
    if (Object.hasOwn(options, key)) throw new Error(`${argv[index]} given twice`);
    options[key] = path.resolve(value);
    index += 1;
  }
  return options;
}

function main(argv) {
  let inputs;
  try {
    inputs = loadInputs(parseArgs(argv));
  } catch (error) {
    console.error(`ERROR: ${error.message}`);
    return 1;
  }
  const errors = validateProbeAuthorities(inputs);
  if (errors.length) {
    console.error(errors.map((error) => `ERROR: ${error}`).join("\n"));
    return 1;
  }
  const cells = inputs.manifests.reduce(
    (total, { doc }) => total + (Array.isArray(doc.entries) ? doc.entries.length : 0),
    0,
  );
  const rows = inputs.observations.reduce((total, { doc }) => total + doc.rows.length, 0);
  console.log(
    `validate-probe-authorities: PASS authorities=${inputs.catalog.authority.length} manifests=${inputs.manifests.length} cells=${cells} joins=${inputs.joins.length} observation_files=${inputs.observations.length} observation_rows=${rows}`,
  );
  return 0;
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url))
  process.exit(main(process.argv.slice(2)));
