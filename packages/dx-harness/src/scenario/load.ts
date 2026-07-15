/**
 * The scenario corpus LOADER: read authored `scenarios/*.yaml`, parse the
 * YAML-subset ({@link ./yaml}), and run every candidate through the existing
 * trust-boundary {@link validateScenario}, returning typed {@link Scenario}s.
 *
 * This binds the committed corpus to the {@link Scenario} model: nothing reaches a
 * caller as a `Scenario` without passing the same validator a hand-built scenario
 * would. The loader is deterministic — files are read in sorted order and
 * in-document order is preserved — so loading the corpus twice yields identical
 * scenarios in identical order. A syntactic fault surfaces as {@link YamlParseError};
 * a semantic fault surfaces as {@link ScenarioLoadError} carrying every offending
 * document's validator errors, so an invalid corpus fails loudly rather than
 * silently yielding a malformed `Scenario`.
 *
 * The parser is re-exported here so the loader's public surface is a single module.
 */

import { readFileSync, readdirSync } from "node:fs";
import { fileURLToPath } from "node:url";

import { canonicalizePath, joinCanonical } from "../paths.js";
import type { Scenario } from "./model.js";
import { validateScenario, type ScenarioValidationError } from "./validate.js";
import { YamlParseError, parseScenarioYaml } from "./yaml.js";

export { YamlParseError, parseScenarioYaml } from "./yaml.js";

/** One scenario document that failed validation, with its origin and position. */
export interface ScenarioLoadFailure {
  /** A human-facing source label (file path or in-memory origin). */
  readonly origin: string;
  /** 0-based position of the offending document within its origin. */
  readonly index: number;
  /** The validator's faults for this document. */
  readonly errors: readonly ScenarioValidationError[];
}

/** A semantic load fault: one or more scenario documents failed validation. */
export class ScenarioLoadError extends Error {
  readonly failures: readonly ScenarioLoadFailure[];
  constructor(failures: readonly ScenarioLoadFailure[]) {
    const summary = failures
      .map(
        (f) =>
          `${f.origin}[${f.index}]: ${f.errors.map((e) => `${e.code}@${e.path || "<root>"}`).join(", ")}`,
      )
      .join("; ");
    super(`invalid scenario(s): ${summary}`);
    this.name = "ScenarioLoadError";
    this.failures = failures;
  }
}

/** Canonical absolute path to the committed `scenarios/` directory. */
export function corpusScenariosDir(): string {
  return canonicalizePath(fileURLToPath(new URL("../../scenarios", import.meta.url)));
}

/** Canonical absolute path to the committed `fixtures/hermetic/` directory. */
export function corpusFixturesDir(): string {
  return canonicalizePath(fileURLToPath(new URL("../../fixtures/hermetic", import.meta.url)));
}

/**
 * Normalise a parsed YAML document into the scenario candidate list: a top-level
 * sequence is a list of scenarios; a single mapping is a one-element list. Any
 * other shape (a scalar, `null`, an empty document) carries no scenario.
 */
function candidatesOf(value: unknown, origin: string): unknown[] {
  if (Array.isArray(value)) return value;
  if (typeof value === "object" && value !== null) return [value];
  throw new ScenarioLoadError([
    {
      origin,
      index: 0,
      errors: [
        { code: "scenario_not_an_object", message: "document carries no scenario", path: "" },
      ],
    },
  ]);
}

/**
 * Validate a list of scenario candidates from one origin, returning typed
 * {@link Scenario}s or throwing {@link ScenarioLoadError} with every fault.
 */
function validateCandidates(candidates: readonly unknown[], origin: string): Scenario[] {
  const failures: ScenarioLoadFailure[] = [];
  candidates.forEach((candidate, index) => {
    const result = validateScenario(candidate);
    if (!result.ok) failures.push({ origin, index, errors: result.errors });
  });
  if (failures.length > 0) throw new ScenarioLoadError(failures);
  // Every candidate validated, so each conforms to the Scenario model.
  return candidates as Scenario[];
}

/**
 * Parse `source` as a YAML-subset scenario document and validate every scenario
 * it carries. `origin` labels faults (a file path or an in-memory test label).
 *
 * @throws {YamlParseError} on a syntactic fault.
 * @throws {ScenarioLoadError} if any document fails {@link validateScenario}.
 */
export function loadScenariosFromSource(source: string, origin: string): Scenario[] {
  const parsed = parseScenarioYaml(source);
  return validateCandidates(candidatesOf(parsed, origin), origin);
}

/**
 * Read and load one `scenarios/*.yaml` file.
 *
 * @throws {YamlParseError} | {@link ScenarioLoadError} as {@link loadScenariosFromSource}.
 */
export function loadScenarioFile(filePath: string): Scenario[] {
  return loadScenariosFromSource(readFileSync(filePath, "utf-8"), filePath);
}

/**
 * Load the whole committed scenario corpus from `dir` (default
 * {@link corpusScenariosDir}). `*.yaml` files are read in sorted order and each
 * document is validated; scenario ids must be unique across the corpus.
 *
 * @throws {ScenarioLoadError} if `dir` holds no `*.yaml` files, if any document is
 *   invalid, or if two scenarios share an `id`.
 */
export function loadScenarioCorpus(dir: string = corpusScenariosDir()): Scenario[] {
  const files = readdirSync(dir)
    .filter((name) => name.endsWith(".yaml"))
    .sort();
  if (files.length === 0) {
    throw new ScenarioLoadError([
      {
        origin: dir,
        index: 0,
        errors: [
          { code: "invalid_anchors", message: "no scenario `*.yaml` files found", path: "" },
        ],
      },
    ]);
  }
  const scenarios: Scenario[] = [];
  const seenIds = new Map<string, string>();
  for (const name of files) {
    const filePath = joinCanonical(dir, name);
    // `index` is the offending document's position WITHIN this file (origin-local),
    // matching `ScenarioLoadFailure.index` — not the running corpus-wide count.
    for (const [index, scenario] of loadScenarioFile(filePath).entries()) {
      const prior = seenIds.get(scenario.id);
      if (prior !== undefined) {
        throw new ScenarioLoadError([
          {
            origin: filePath,
            index,
            errors: [
              {
                code: "invalid_scenario_id",
                message: `duplicate scenario id "${scenario.id}" (also in ${prior})`,
                path: "id",
              },
            ],
          },
        ]);
      }
      seenIds.set(scenario.id, name);
      scenarios.push(scenario);
    }
  }
  return scenarios;
}
