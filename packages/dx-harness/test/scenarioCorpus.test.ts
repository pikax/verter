import { existsSync, mkdtempSync, readFileSync, readdirSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { fileURLToPath } from "node:url";

import { describe, expect, it } from "vitest";

import { addFileAnchors, stripAnchors, type AnchorMap } from "../src/anchors.js";
import { canonicalizePath, joinCanonical } from "../src/paths.js";
import {
  ScenarioLoadError,
  corpusFixturesDir,
  loadScenarioCorpus,
  loadScenarioFile,
  loadScenariosFromSource,
  validateScenario,
  type Scenario,
} from "../src/scenario/index.js";
import { ORACLE_FAMILIES } from "../src/semantic-oracle/model.js";

const HERMETIC = canonicalizePath(fileURLToPath(new URL("../fixtures/hermetic", import.meta.url)));
const SCENARIOS = canonicalizePath(fileURLToPath(new URL("../scenarios", import.meta.url)));
const TEXT_SOURCE = /\.(vue|ts|tsx|js|jsx|mts|cts)$/;

/** The eight named scenario files — the committed scenario file layout. */
const SCENARIO_FILES = [
  "auto-import.yaml",
  "churn.yaml",
  "completion.yaml",
  "definition.yaml",
  "diagnostics.yaml",
  "hover.yaml",
  "recovery.yaml",
  "semantic-oracle.yaml",
] as const;

/**
 * The documented scenario count per file. The corpus is eight files / fifteen
 * scenarios; pinning each file's count means adding or dropping a scenario in ANY
 * file — including one of the curated semantic-oracle documents — fails the gate.
 * The semantic-oracle count is derived from the oracle family list so a dropped
 * family moves it in lockstep; {@link TOTAL_SCENARIOS} is pinned independently.
 */
const SCENARIOS_PER_FILE: Record<(typeof SCENARIO_FILES)[number], number> = {
  "auto-import.yaml": 1,
  "churn.yaml": 1,
  "completion.yaml": 1,
  "definition.yaml": 1,
  "diagnostics.yaml": 1,
  "hover.yaml": 1,
  "recovery.yaml": 1,
  "semantic-oracle.yaml": ORACLE_FAMILIES.length,
};

/** The pinned corpus total: eight files, fifteen scenarios. */
const TOTAL_SCENARIOS = 15;

function walk(root: string, rel = "", out: string[] = []): string[] {
  const here = joinCanonical(root, rel);
  for (const entry of readdirSync(here, { withFileTypes: true })) {
    if (entry.name.startsWith(".")) continue;
    const childRel = joinCanonical(rel, entry.name);
    if (entry.isDirectory()) walk(root, childRel, out);
    else if (entry.isFile()) out.push(childRel);
  }
  return out;
}

/** Build the materializer-equivalent anchor set for a fixture id. */
function fixtureAnchorSet(fixture: string): Set<string> {
  const dir = joinCanonical(HERMETIC, fixture);
  const anchors: AnchorMap = new Map();
  for (const rel of walk(dir).sort()) {
    if (!TEXT_SOURCE.test(rel)) continue;
    addFileAnchors(anchors, rel, stripAnchors(readFileSync(joinCanonical(dir, rel), "utf-8")));
  }
  return new Set(anchors.keys());
}

/** All anchor names a scenario references (declared, probes, edits, invariants). */
function referencedAnchors(s: Scenario): string[] {
  return [
    ...s.anchors,
    ...s.probes.map((p) => p.anchor),
    ...(s.setup ?? []).map((e) => e.anchor),
    ...s.script.map((e) => e.anchor),
    ...s.invariants.map((i) => i.anchor),
  ];
}

describe("scenario corpus — every committed scenario loads and validates", () => {
  it("ships exactly the eight named scenario files", () => {
    const files = readdirSync(SCENARIOS)
      .filter((f) => f.endsWith(".yaml"))
      .sort();
    expect(files).toEqual([...SCENARIO_FILES]);
  });

  it("corpusFixturesDir resolves to the committed fixtures/hermetic root", () => {
    const dir = corpusFixturesDir();
    expect(existsSync(dir), "fixtures/hermetic exists on disk").toBe(true);
    // Canonical, and the same root this suite walks for anchors.
    expect(dir).toBe(HERMETIC);
    // It is the real corpus root: the named fixtures live directly under it.
    expect(readdirSync(dir)).toContain("semantic-oracle");
  });

  it("loads EXACTLY fifteen scenarios — the documented eight-file corpus", () => {
    // The per-file expectations must themselves sum to the independently-pinned
    // total, so the two pins cannot silently drift apart.
    const sum = Object.values(SCENARIOS_PER_FILE).reduce((a, b) => a + b, 0);
    expect(sum, "per-file counts sum to the pinned total").toBe(TOTAL_SCENARIOS);
    expect(loadScenarioCorpus().length, "corpus loads exactly fifteen scenarios").toBe(
      TOTAL_SCENARIOS,
    );
  });

  it("loads the documented scenario count from each file", () => {
    for (const name of SCENARIO_FILES) {
      const loaded = loadScenarioFile(joinCanonical(SCENARIOS, name));
      expect(loaded.length, `${name} scenario count`).toBe(SCENARIOS_PER_FILE[name]);
    }
  });

  it("loads the whole corpus through validateScenario with ZERO errors", () => {
    for (const scenario of loadScenarioCorpus()) {
      const result = validateScenario(scenario);
      expect(result.errors, `scenario "${scenario.id}" validates`).toEqual([]);
      expect(result.ok).toBe(true);
    }
  });

  it("assigns every scenario a unique id and a known fixture id", () => {
    const scenarios = loadScenarioCorpus();
    const ids = scenarios.map((s) => s.id);
    expect(new Set(ids).size, "ids are unique").toBe(ids.length);
    for (const s of scenarios) {
      expect(readdirSync(HERMETIC), `fixture "${s.fixture}" exists`).toContain(s.fixture);
    }
  });
});

describe("scenario corpus — probe/edit/invariant anchors exist in the referenced fixture", () => {
  it("cross-checks every referenced anchor against the fixture's authored anchor set", () => {
    for (const scenario of loadScenarioCorpus()) {
      const declared = fixtureAnchorSet(scenario.fixture);
      for (const anchor of referencedAnchors(scenario)) {
        expect(declared.has(anchor), `"${anchor}" exists in fixture "${scenario.fixture}"`).toBe(
          true,
        );
      }
      // The entry file is a real `.vue` in that fixture.
      expect(walk(joinCanonical(HERMETIC, scenario.fixture))).toContain(scenario.entryFile);
    }
  });
});

describe("scenario corpus — deterministic load", () => {
  it("loading the corpus twice yields identical scenarios in identical order", () => {
    expect(loadScenarioCorpus()).toEqual(loadScenarioCorpus());
  });
});

describe("scenario corpus — no dangling declared anchors", () => {
  it("references every declared anchor from a probe, edit step, or invariant", () => {
    // A name declared in `anchors` but used by nothing is dead weight that the
    // cross-check above cannot catch (it only validates the other direction).
    for (const scenario of loadScenarioCorpus()) {
      const used = new Set<string>([
        ...scenario.probes.map((p) => p.anchor),
        ...(scenario.setup ?? []).map((e) => e.anchor),
        ...scenario.script.map((e) => e.anchor),
        ...scenario.invariants.map((i) => i.anchor),
      ]);
      for (const declared of scenario.anchors) {
        expect(
          used.has(declared),
          `anchor "${declared}" in scenario "${scenario.id}" is referenced`,
        ).toBe(true);
      }
    }
  });
});

// A minimal valid scenario, authored in the same YAML subset the corpus uses. The
// rejection tests mutate ONE field each, so a green control proves the mutation —
// not some unrelated breakage — is what the loader catches.
const VALID = [
  "- id: control-scenario",
  "  fixture: minimal-member-access",
  "  entryFile: App.vue",
  "  anchors: [mma.member]",
  "  script: []",
  "  probes:",
  "    - id: p1",
  "      method: completion",
  "      anchor: mma.member",
  "      mappingPolicy: strict",
  "      confidence: high",
  "      dimension: artifactParity",
  "      requiresSourceMap: true",
  "      requiredDrivers: [rawLsp, tsgo]",
  "      capabilityRequirements: [positionEncoding]",
  "  invariants: []",
  "  baselines:",
  "    tsgo: required",
  "    tsserver: requiredForCi",
  "    volar: optional",
  "  thresholds:",
  "    flakeWindows: 2",
].join("\n");

describe("scenario loader — rejects malformed scenarios (no rubber-stamp)", () => {
  it("accepts the valid control with zero errors", () => {
    const loaded = loadScenariosFromSource(VALID, "control");
    expect(loaded).toHaveLength(1);
    expect(loaded[0].id).toBe("control-scenario");
  });

  /** Run the loader, returning the aggregated validator error codes (or throwing if it did NOT reject). */
  function rejectionCodes(source: string): string[] {
    try {
      loadScenariosFromSource(source, "mutated");
      throw new Error("expected ScenarioLoadError, loader accepted the scenario");
    } catch (err) {
      expect(err, "is a ScenarioLoadError").toBeInstanceOf(ScenarioLoadError);
      return (err as ScenarioLoadError).failures.flatMap((f) => f.errors.map((e) => e.code));
    }
  }

  it("rejects a probe anchor not declared in the scenario's anchors", () => {
    const mutated = VALID.replace("anchor: mma.member", "anchor: ghost-anchor");
    expect(rejectionCodes(mutated)).toContain("probe_anchor_undeclared");
  });

  it("rejects an unknown probe method (bad enum)", () => {
    const mutated = VALID.replace("method: completion", "method: frobnicate");
    expect(rejectionCodes(mutated)).toContain("invalid_method");
  });

  it("rejects a confidence above the mapping-policy ceiling", () => {
    const mutated = VALID.replace(
      "mappingPolicy: strict",
      "mappingPolicy: nearestTokenLowConfidence",
    );
    // strict→requiresSourceMap is still fine; nearestToken caps confidence at `low`,
    // so the unchanged `confidence: high` now exceeds the ceiling.
    expect(rejectionCodes(mutated)).toContain("confidence_exceeds_mapping_policy_ceiling");
  });

  it("rejects a scenario whose required thresholds are missing", () => {
    const mutated = VALID.replace(/  thresholds:\n    flakeWindows: 2/, "");
    expect(rejectionCodes(mutated)).toContain("invalid_thresholds");
  });

  it("rejects a requiresSourceMap inconsistent with the mapping policy", () => {
    const mutated = VALID.replace("requiresSourceMap: true", "requiresSourceMap: false");
    expect(rejectionCodes(mutated)).toContain("mapping_policy_requires_source_map");
  });
});

describe("scenario corpus — duplicate id across files", () => {
  it("reports the offending document's index WITHIN its own file, not a corpus-global count", () => {
    // Two single-scenario files share an id. The duplicate sits at position 0 of the
    // SECOND file, so the reported `index` must be 0 (origin-local), never 1 (the
    // running corpus-wide scenario count after the first file).
    const oneScenario = (id: string): string => VALID.replace("control-scenario", id);
    const dir = canonicalizePath(
      mkdtempSync(joinCanonical(canonicalizePath(tmpdir()), "dx-corpus-")),
    );
    try {
      writeFileSync(joinCanonical(dir, "a.yaml"), oneScenario("shared-id"), "utf-8");
      writeFileSync(joinCanonical(dir, "b.yaml"), oneScenario("shared-id"), "utf-8");
      let caught: unknown;
      try {
        loadScenarioCorpus(dir);
      } catch (err) {
        caught = err;
      }
      expect(caught, "duplicate id across files throws").toBeInstanceOf(ScenarioLoadError);
      const failures = (caught as ScenarioLoadError).failures;
      expect(failures).toHaveLength(1);
      expect(failures[0].index, "origin-local index of the duplicate document").toBe(0);
      expect(failures[0].origin).toContain("b.yaml");
      expect(failures[0].errors[0].code).toBe("invalid_scenario_id");
    } finally {
      rmSync(dir, { recursive: true, force: true });
    }
  });
});
