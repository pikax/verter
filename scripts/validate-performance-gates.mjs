#!/usr/bin/env node
// Performance-gate file validator. Locked file must set status = "LOCKED",
// contain no REQUIRED_* values, and pass this script.
//
//   node scripts/validate-performance-gates.mjs --gates <performance-gates.toml>
//
// Exit: 0 pass, 1 validation failure (one violation per line), 2 usage /
// unreadable input. Unknown TOML is a loud failure, never a silent skip.

import { readFileSync } from "node:fs";
import process from "node:process";

// Minimal strict TOML reader.
//
// Supported shapes: full-line comments, `[table]`, `[[array-of-tables]]`, and
// `key = value` where value is a basic double-quoted string (no escapes), an
// array of basic strings (single- or multi-line), an integer, a float, or a
// boolean. A trailing `# comment` after a value is allowed, except inside a
// string. Everything else fails loudly with the file/line.

class TomlError extends Error {}

function stripTrailingComment(raw) {
  let inString = false;
  for (let i = 0; i < raw.length; i += 1) {
    const ch = raw[i];
    if (ch === '"') inString = !inString;
    else if (ch === "#" && !inString) return raw.slice(0, i);
  }
  return raw;
}

function parseScalar(text, lineNo) {
  const value = text.trim();
  if (value === "true") return true;
  if (value === "false") return false;
  if (/^"[^"\\]*"$/.test(value)) return value.slice(1, -1);
  if (/^-?\d+$/.test(value)) return Number.parseInt(value, 10);
  if (/^-?\d+\.\d+$/.test(value)) return Number.parseFloat(value);
  throw new TomlError(`line ${lineNo}: unsupported value \`${value}\``);
}

function parseArray(text, lineNo) {
  const inner = text.trim().slice(1, -1).trim();
  if (inner === "") return [];
  const parts = [];
  let depth = 0;
  let inString = false;
  let current = "";
  for (const ch of inner) {
    if (ch === '"') inString = !inString;
    if (!inString && ch === "[") depth += 1;
    if (!inString && ch === "]") depth -= 1;
    if (ch === "," && depth === 0 && !inString) {
      parts.push(current);
      current = "";
    } else {
      current += ch;
    }
  }
  if (current.trim() !== "") parts.push(current);
  return parts.map((part) => parseScalar(part, lineNo));
}

// Returns { root, cells } where `root` holds every non-`[[cell]]` table keyed by
// its dotted header and `cells` is the ordered array-of-tables. `[cell.x]`
// headers attach to the most recently opened cell.
export function readGatesToml(text) {
  const root = { "": {} };
  const cells = [];
  let current = root[""];
  let currentCell = null;
  const lines = text.split(/\r?\n/);

  for (let i = 0; i < lines.length; i += 1) {
    let raw = lines[i];
    const lineNo = i + 1;
    let line = stripTrailingComment(raw).trim();
    if (line === "") continue;

    if (line.startsWith("[[")) {
      if (!line.endsWith("]]"))
        throw new TomlError(`line ${lineNo}: malformed array-of-tables header`);
      const name = line.slice(2, -2).trim();
      if (name === "cell") {
        currentCell = { __metrics: [] };
        cells.push(currentCell);
        current = currentCell;
      } else if (name === "cell.metric") {
        if (currentCell === null)
          throw new TomlError(`line ${lineNo}: [[cell.metric]] before any [[cell]]`);
        const metric = {};
        currentCell.__metrics.push(metric);
        current = metric;
      } else {
        throw new TomlError(`line ${lineNo}: unsupported array-of-tables \`${name}\``);
      }
      continue;
    }

    if (line.startsWith("[")) {
      if (!line.endsWith("]")) throw new TomlError(`line ${lineNo}: malformed table header`);
      const name = line.slice(1, -1).trim();
      if (name.startsWith("cell.")) {
        if (currentCell === null)
          throw new TomlError(`line ${lineNo}: [${name}] before any [[cell]]`);
        currentCell[name] = currentCell[name] ?? {};
        current = currentCell[name];
      } else {
        root[name] = root[name] ?? {};
        current = root[name];
      }
      continue;
    }

    const eq = line.indexOf("=");
    if (eq < 0) throw new TomlError(`line ${lineNo}: not a key/value pair: \`${line}\``);
    const key = line.slice(0, eq).trim();
    let valueText = line.slice(eq + 1).trim();

    // Multi-line array: keep consuming raw lines until the brackets balance.
    if (valueText.startsWith("[") && !valueText.endsWith("]")) {
      let depth = 0;
      const consume = (chunk) => {
        for (const ch of chunk) {
          if (ch === "[") depth += 1;
          if (ch === "]") depth -= 1;
        }
      };
      consume(valueText);
      while (depth > 0) {
        i += 1;
        if (i >= lines.length) throw new TomlError(`line ${lineNo}: unterminated array`);
        const next = stripTrailingComment(lines[i]).trim();
        consume(next);
        valueText += next;
      }
    }

    current[key] = valueText.startsWith("[")
      ? parseArray(valueText, lineNo)
      : parseScalar(valueText, lineNo);
  }

  return { root, cells };
}

// Validation.

const BOUNDARIES = new Set(["rust", "napi", "wasm", "lsp", "cli"]);
const STATISTICS = new Set(["median", "max", "min", "mean", "p95", "p99"]);
const COMPARISONS = new Set(["absolute_max", "absolute_min", "no_regression_percent_max"]);
const COMPETITOR_RULES = new Set(["none", "pareto", "suite_geomean", "both"]);
const NOT_APPLICABLE = "not_applicable";

// Template field lists. A locked file that silently drops a field is not a
// locked instance of the template.
const RUNNER_KEYS = [
  "class",
  "os",
  "cpu",
  "logical_cpus",
  "memory_bytes",
  "rust_toolchain",
  "node_runtime",
  "power_policy",
  "control_benchmark",
  "max_control_drift_percent",
];
const STATISTICS_KEYS = [
  "short_min_samples",
  "long_min_runs",
  "confidence",
  "bootstrap_resamples",
  "no_regression_floor_percent",
  "noise_multiplier",
  "outlier_policy",
  "interleave_policy",
];
const CELL_KEYS = [
  "id",
  "owner",
  "operation",
  "corpus_fingerprint",
  "normalized_product_request_digest",
  "result_contract",
  "semantic_profile",
  "execution_profile",
  "cache_state",
  "threads",
  "boundary",
  "required",
];
const VALIDITY_KEYS = [
  "required_product_kinds",
  "required_output_profiles",
  "required_presentation_profiles",
  "required_serialization_profiles",
  "required_mapping_kinds",
  "required_diagnostics_policy",
  "required_exactness",
  "output_oracle",
  "zero_counter_assertions",
];
const MEMORY_KEYS = [
  "owner_budget_bytes",
  "allocator_slack_bytes",
  "quiescence_protocol",
  "max_positive_slope_bytes_per_hour",
];

const isString = (v) => typeof v === "string";
const isNumber = (v) => typeof v === "number" && Number.isFinite(v);
const isInt = (v) => Number.isInteger(v);

// The core rule (template header line 3): no REQUIRED_* value survives into a
// locked file. Checked over every string value anywhere in the document, plus
// the generic PLACEHOLDER spelling, because a placeholder that renamed itself is
// still a placeholder.
const PLACEHOLDER = /REQUIRED_|PLACEHOLDER|\bTBD\b|\bTODO\b|\bFIXME\b|UNDECIDED/i;

function walkStrings(node, path, visit) {
  if (isString(node)) {
    visit(node, path);
  } else if (Array.isArray(node)) {
    node.forEach((item, index) => walkStrings(item, `${path}[${index}]`, visit));
  } else if (node && typeof node === "object") {
    for (const [key, value] of Object.entries(node)) {
      walkStrings(value, path === "" ? key : `${path}.${key}`, visit);
    }
  }
}

export function validateGates(text) {
  const violations = [];
  const fail = (message) => violations.push(message);

  let doc;
  try {
    doc = readGatesToml(text);
  } catch (error) {
    return { violations: [`TOML: ${error.message}`], cells: 0, metrics: 0 };
  }

  const { root, cells } = doc;
  const top = root[""];

  // placeholders, anywhere
  walkStrings(top, "", (value, path) => {
    if (PLACEHOLDER.test(value)) fail(`placeholder value at \`${path}\`: \`${value}\``);
  });
  for (const [name, table] of Object.entries(root)) {
    if (name === "") continue;
    walkStrings(table, name, (value, path) => {
      if (PLACEHOLDER.test(value)) fail(`placeholder value at \`${path}\`: \`${value}\``);
    });
  }
  cells.forEach((cell, index) => {
    const label = isString(cell.id) ? cell.id : `#${index + 1}`;
    walkStrings({ ...cell, __metrics: undefined }, `cell[${label}]`, (value, path) => {
      if (PLACEHOLDER.test(value)) fail(`placeholder value at \`${path}\`: \`${value}\``);
    });
    cell.__metrics.forEach((metric, mIndex) => {
      walkStrings(metric, `cell[${label}].metric[${mIndex + 1}]`, (value, path) => {
        if (PLACEHOLDER.test(value)) fail(`placeholder value at \`${path}\`: \`${value}\``);
      });
    });
  });

  // document header
  if (top.schema !== 1) fail("top-level `schema` must be the integer 1");
  if (top.revision !== 11) fail("top-level `revision` must be the integer 11");
  if (top.status !== "LOCKED") fail('top-level `status` must be "LOCKED"');
  if (!isString(top.authority_digest) || top.authority_digest.trim() === "")
    fail("top-level `authority_digest` must be a non-empty string");
  if (!isString(top.baseline_sha) || !/^[0-9a-f]{40}$/.test(top.baseline_sha))
    fail("top-level `baseline_sha` must be a full 40-hex commit SHA");
  if (
    !isString(top.created_at_utc) ||
    !/^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}Z$/.test(top.created_at_utc)
  )
    fail("top-level `created_at_utc` must be an ISO-8601 UTC instant (…Z)");

  // [runner]
  const runner = root.runner;
  if (!runner) {
    fail("missing `[runner]` table");
  } else {
    for (const key of RUNNER_KEYS) {
      if (!(key in runner)) fail(`\`[runner]\` is missing \`${key}\``);
    }
    if (!isInt(runner.logical_cpus) || runner.logical_cpus <= 0)
      fail("`runner.logical_cpus` must be a positive integer");
    if (!isInt(runner.memory_bytes) || runner.memory_bytes <= 0)
      fail("`runner.memory_bytes` must be a positive integer");
    if (!isNumber(runner.max_control_drift_percent) || runner.max_control_drift_percent <= 0)
      fail("`runner.max_control_drift_percent` must be a positive number, not a string");
    for (const key of [
      "class",
      "os",
      "cpu",
      "rust_toolchain",
      "node_runtime",
      "power_policy",
      "control_benchmark",
    ]) {
      if (key in runner && (!isString(runner[key]) || runner[key].trim() === ""))
        fail(`\`runner.${key}\` must be a non-empty string`);
    }
  }

  // [statistics]
  // These bounds are part of the checked-in performance-gate contract.
  const stats = root.statistics;
  if (!stats) {
    fail("missing `[statistics]` table");
  } else {
    for (const key of STATISTICS_KEYS) {
      if (!(key in stats)) fail(`\`[statistics]\` is missing \`${key}\``);
    }
    if (!isInt(stats.short_min_samples) || stats.short_min_samples < 30)
      fail("`statistics.short_min_samples` must be an integer >= 30 (verification.md 8.3)");
    if (!isInt(stats.long_min_runs) || stats.long_min_runs < 10)
      fail("`statistics.long_min_runs` must be an integer >= 10 (verification.md 8.3)");
    if (stats.confidence !== 0.95)
      fail("`statistics.confidence` must be 0.95 (verification.md 8.3)");
    if (!isInt(stats.bootstrap_resamples) || stats.bootstrap_resamples < 10000)
      fail("`statistics.bootstrap_resamples` must be an integer >= 10000 (verification.md 8.3)");
    if (!isNumber(stats.no_regression_floor_percent) || stats.no_regression_floor_percent <= 0)
      fail("`statistics.no_regression_floor_percent` must be a positive number");
    if (!isNumber(stats.noise_multiplier) || stats.noise_multiplier <= 0)
      fail("`statistics.noise_multiplier` must be a positive number");
    for (const key of ["outlier_policy", "interleave_policy"]) {
      if (key in stats && (!isString(stats[key]) || stats[key].trim() === ""))
        fail(`\`statistics.${key}\` must be a non-empty predeclared policy string`);
    }
  }

  // cells
  if (cells.length === 0) fail("no `[[cell]]` is declared");
  const cellIds = new Set();
  let metricCount = 0;

  for (const [index, cell] of cells.entries()) {
    const label = isString(cell.id) && cell.id !== "" ? cell.id : `#${index + 1}`;
    for (const key of CELL_KEYS) {
      if (!(key in cell)) fail(`cell \`${label}\` is missing \`${key}\``);
    }
    if (!isString(cell.id) || cell.id.trim() === "")
      fail(`cell ${label}: \`id\` must be a non-empty string`);
    else if (cellIds.has(cell.id)) fail(`duplicate cell id \`${cell.id}\``);
    else cellIds.add(cell.id);

    for (const key of [
      "owner",
      "operation",
      "corpus_fingerprint",
      "normalized_product_request_digest",
      "result_contract",
      "semantic_profile",
      "execution_profile",
      "cache_state",
    ]) {
      if (key in cell && (!isString(cell[key]) || cell[key].trim() === ""))
        fail(`cell ${label}: \`${key}\` must be a non-empty string`);
    }
    if (!isInt(cell.threads) || cell.threads <= 0)
      fail(`cell ${label}: \`threads\` must be a positive integer, not a string`);
    if (!BOUNDARIES.has(cell.boundary))
      fail(`cell ${label}: \`boundary\` must be one of ${[...BOUNDARIES].join("|")}`);
    if (typeof cell.required !== "boolean") fail(`cell ${label}: \`required\` must be a boolean`);

    // [cell.validity]
    const validity = cell["cell.validity"];
    if (!validity) {
      fail(`cell ${label}: missing \`[cell.validity]\``);
    } else {
      for (const key of VALIDITY_KEYS) {
        if (!(key in validity)) fail(`cell ${label}: \`[cell.validity]\` is missing \`${key}\``);
      }
      for (const key of [
        "required_product_kinds",
        "required_output_profiles",
        "required_presentation_profiles",
        "required_serialization_profiles",
        "required_mapping_kinds",
        "zero_counter_assertions",
      ]) {
        if (key in validity && !Array.isArray(validity[key]))
          fail(`cell ${label}: \`validity.${key}\` must be an array`);
      }
      if (
        Array.isArray(validity.required_product_kinds) &&
        validity.required_product_kinds.length === 0
      )
        fail(
          `cell ${label}: \`validity.required_product_kinds\` must name at least one product kind`,
        );
      for (const key of ["required_diagnostics_policy", "required_exactness", "output_oracle"]) {
        if (key in validity && (!isString(validity[key]) || validity[key].trim() === ""))
          fail(`cell ${label}: \`validity.${key}\` must be a non-empty string`);
      }
    }

    // [[cell.metric]]
    const metrics = cell.__metrics;
    if (metrics.length === 0) {
      fail(`cell ${label}: declares no \`[[cell.metric]]\``);
    }
    let hasAbsolute = false;
    let hasRelative = false;
    for (const [mIndex, metric] of metrics.entries()) {
      const mLabel = `cell ${label} metric #${mIndex + 1}`;
      metricCount += 1;
      if (!isString(metric.name) || metric.name.trim() === "")
        fail(`${mLabel}: \`name\` must be a non-empty string`);
      if (!STATISTICS.has(metric.statistic))
        fail(`${mLabel}: \`statistic\` must be one of ${[...STATISTICS].join("|")}`);
      if (!COMPARISONS.has(metric.comparison))
        fail(`${mLabel}: \`comparison\` must be one of ${[...COMPARISONS].join("|")}`);
      // The load-bearing type check: an unresolved limit is a string.
      if (!isNumber(metric.limit))
        fail(`${mLabel} (\`${metric.name}\`): \`limit\` must be a number, not a string`);
      else if (metric.limit < 0)
        fail(`${mLabel} (\`${metric.name}\`): \`limit\` must not be negative`);
      if (metric.comparison === "absolute_max" || metric.comparison === "absolute_min")
        hasAbsolute = true;
      if (metric.comparison === "no_regression_percent_max") {
        hasRelative = true;
        if (isNumber(metric.limit) && metric.limit <= 0)
          fail(
            `${mLabel} (\`${metric.name}\`): a no-regression bound of ${metric.limit} admits nothing and gates nothing`,
          );
      }
    }
    // verification.md 8.2: a cell fixes "absolute and relative gate".
    if (cell.required === true && !hasAbsolute)
      fail(`cell ${label}: required cell declares no absolute gate (verification.md 8.2)`);
    if (cell.required === true && !hasRelative)
      fail(`cell ${label}: required cell declares no no-regression gate (verification.md 8.2)`);

    // [cell.competitor]
    const competitor = cell["cell.competitor"];
    if (!competitor) {
      fail(`cell ${label}: missing \`[cell.competitor]\``);
    } else {
      if (!COMPETITOR_RULES.has(competitor.rule))
        fail(
          `cell ${label}: \`competitor.rule\` must be one of ${[...COMPETITOR_RULES].join("|")}`,
        );
      if (!Array.isArray(competitor.competitor_ids))
        fail(`cell ${label}: \`competitor.competitor_ids\` must be an array`);
      const comparing = competitor.rule !== undefined && competitor.rule !== "none";
      if (
        comparing &&
        Array.isArray(competitor.competitor_ids) &&
        competitor.competitor_ids.length === 0
      )
        fail(`cell ${label}: \`competitor.rule = "${competitor.rule}"\` names no competitor`);
      for (const key of ["max_wall_slowdown_percent", "max_peak_rss_increase_percent"]) {
        const value = competitor[key];
        if (value === undefined) fail(`cell ${label}: \`[cell.competitor]\` is missing \`${key}\``);
        else if (comparing && !isNumber(value))
          fail(
            `cell ${label}: \`competitor.${key}\` must be a number when a competitor rule is active`,
          );
        else if (!comparing && !isNumber(value) && value !== NOT_APPLICABLE)
          fail(`cell ${label}: \`competitor.${key}\` must be a number or "${NOT_APPLICABLE}"`);
      }
      // ADR-016: a candidate cannot choose its pass criterion after measurement.
      if (competitor.post_result_exception_allowed !== false)
        fail(`cell ${label}: \`competitor.post_result_exception_allowed\` must be false (ADR-016)`);
    }

    // [cell.memory]
    const memory = cell["cell.memory"];
    if (!memory) {
      fail(`cell ${label}: missing \`[cell.memory]\``);
    } else {
      for (const key of MEMORY_KEYS) {
        if (!(key in memory)) fail(`cell ${label}: \`[cell.memory]\` is missing \`${key}\``);
        else {
          const value = memory[key];
          if (!isNumber(value) && value !== NOT_APPLICABLE)
            fail(`cell ${label}: \`memory.${key}\` must be a number or "${NOT_APPLICABLE}"`);
        }
      }
    }
  }

  // [primary_suite]
  const suite = root.primary_suite;
  if (!suite) {
    fail("missing `[primary_suite]` table");
  } else {
    if (!isString(suite.id) || suite.id.trim() === "")
      fail("`primary_suite.id` must be a non-empty string");
    if (!Array.isArray(suite.cell_ids) || suite.cell_ids.length === 0)
      fail("`primary_suite.cell_ids` must name at least one cell");
    else {
      for (const id of suite.cell_ids) {
        if (!cellIds.has(id))
          fail(`\`primary_suite.cell_ids\` names \`${id}\`, which no [[cell]] declares`);
      }
    }
    if (!isString(suite.aggregate) || suite.aggregate.trim() === "")
      fail("`primary_suite.aggregate` must declare an aggregation rule");
    if (!Array.isArray(suite.competitor_ids))
      fail("`primary_suite.competitor_ids` must be an array");
    const suiteComparing = Array.isArray(suite.competitor_ids) && suite.competitor_ids.length > 0;
    if (suite.max_verter_to_fastest_ratio === undefined)
      fail("`primary_suite` is missing `max_verter_to_fastest_ratio`");
    else if (suiteComparing && !isNumber(suite.max_verter_to_fastest_ratio))
      fail(
        "`primary_suite.max_verter_to_fastest_ratio` must be a number when competitors are named",
      );
    else if (
      !suiteComparing &&
      !isNumber(suite.max_verter_to_fastest_ratio) &&
      suite.max_verter_to_fastest_ratio !== NOT_APPLICABLE
    )
      fail(`\`primary_suite.max_verter_to_fastest_ratio\` must be a number or "${NOT_APPLICABLE}"`);
    if (suite.post_result_exception_allowed !== false)
      fail("`primary_suite.post_result_exception_allowed` must be false (ADR-016)");
    if (suite.premise_change_requires_new_lock !== true)
      fail("`primary_suite.premise_change_requires_new_lock` must be true (ADR-016)");
  }

  if (!cells.some((cell) => cell.required === true))
    fail("no cell is `required = true`; the file gates nothing");

  return { violations, cells: cells.length, metrics: metricCount };
}

// CLI

function main(argv) {
  const index = argv.indexOf("--gates");
  if (index < 0 || index + 1 >= argv.length) {
    process.stderr.write(
      "usage: node scripts/validate-performance-gates.mjs --gates <performance-gates.toml>\n",
    );
    return 2;
  }
  const path = argv[index + 1];
  let text;
  try {
    text = readFileSync(path, "utf8");
  } catch (error) {
    process.stderr.write(`cannot read ${path}: ${error.message}\n`);
    return 2;
  }

  const { violations, cells, metrics } = validateGates(text);
  if (violations.length > 0) {
    for (const violation of violations) process.stdout.write(`FAIL ${violation}\n`);
    process.stdout.write(`FAIL ${path}: ${violations.length} violation(s)\n`);
    return 1;
  }
  process.stdout.write(`PASS ${path}: ${cells} cell(s), ${metrics} metric(s), no placeholders\n`);
  return 0;
}

if (import.meta.url === `file://${process.argv[1]}`) {
  process.exit(main(process.argv.slice(2)));
}
