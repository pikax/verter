/**
 * The {@link Scenario} trust-boundary validator.
 *
 * Scenarios are authored as data (YAML/JSON), so the validator accepts `unknown`
 * and is TOTAL — it never throws, it accumulates typed, discriminable errors. It
 * catches the structural faults: duplicate probe ids; an anchor reference
 * (probe/edit/invariant) absent from the declared {@link Scenario.anchors}; an
 * invalid closed-enum value; an inconsistent `requiresSourceMap` against the
 * mapping policy; a method whose dimension is structurally impossible; an invariant
 * whose method has no Vue surface; and a declared confidence above the
 * mapping-policy ceiling.
 *
 * Cross-field rules run only when their component enum fields are themselves
 * valid, so one bad enum produces one focused error instead of a cascade.
 */

import {
  confidenceWithinCeiling,
  isBaselineRequirement,
  isConfidence,
  isEditStepKind,
  isInvariantAssertion,
  isMappingPolicy,
  isProbeMethod,
  isRequiredDriver,
  isSemanticDimension,
  mappingPolicyRequiresSourceMap,
  methodSupportsDimension,
  methodSupportsInvariant,
} from "./model.js";

/** Every discriminable validation fault code the validator can emit. */
export const SCENARIO_ERROR_CODES = [
  "scenario_not_an_object",
  "invalid_scenario_id",
  "invalid_fixture",
  "invalid_entry_file",
  "invalid_anchors",
  "duplicate_anchor_declaration",
  "invalid_probes",
  "invalid_probe",
  "invalid_probe_id",
  "duplicate_probe_id",
  "invalid_probe_anchor",
  "probe_anchor_undeclared",
  "invalid_method",
  "invalid_mapping_policy",
  "invalid_confidence",
  "invalid_dimension",
  "invalid_requires_source_map",
  "invalid_required_drivers",
  "invalid_required_driver",
  "invalid_capability_requirements",
  "invalid_capability_requirement",
  "source_map_required_but_mapping_policy_none",
  "mapping_policy_requires_source_map",
  "method_dimension_structurally_impossible",
  "confidence_exceeds_mapping_policy_ceiling",
  "invalid_setup",
  "invalid_script",
  "invalid_edit_step",
  "invalid_edit_step_kind",
  "invalid_edit_step_anchor",
  "edit_step_anchor_undeclared",
  "invalid_edit_step_text",
  "invalid_edit_step_remove_units",
  "burst_unsupported_for_step_kind",
  "invalid_invariants",
  "invalid_invariant",
  "invalid_invariant_id",
  "duplicate_invariant_id",
  "invalid_invariant_anchor",
  "invariant_anchor_undeclared",
  "invalid_invariant_method",
  "invariant_method_not_a_surface",
  "invalid_invariant_assertion",
  "invalid_invariant_value",
  "invalid_baselines",
  "invalid_baseline_requirement",
  "invalid_thresholds",
  "invalid_threshold_field",
  "invalid_threshold",
  "negative_threshold",
] as const;
export type ScenarioErrorCode = (typeof SCENARIO_ERROR_CODES)[number];

/** One validation fault: a discriminable code, a human message, and a JSON-path locator. */
export interface ScenarioValidationError {
  readonly code: ScenarioErrorCode;
  readonly message: string;
  /** Dotted/indexed path to the offending element, e.g. `probes[2].anchor`. */
  readonly path: string;
}

/** The validation verdict over a candidate scenario. */
export interface ScenarioValidationResult {
  readonly ok: boolean;
  readonly errors: readonly ScenarioValidationError[];
}

function asRecord(value: unknown): Record<string, unknown> | undefined {
  return typeof value === "object" && value !== null && !Array.isArray(value)
    ? (value as Record<string, unknown>)
    : undefined;
}

function isNonEmptyString(value: unknown): value is string {
  return typeof value === "string" && value.length > 0;
}

class ErrorSink {
  readonly errors: ScenarioValidationError[] = [];
  push(code: ScenarioErrorCode, message: string, path: string): void {
    this.errors.push({ code, message, path });
  }
}

function validateProbes(raw: unknown, anchors: ReadonlySet<string>, sink: ErrorSink): void {
  if (!Array.isArray(raw)) {
    sink.push("invalid_probes", "`probes` must be an array", "probes");
    return;
  }
  const seenIds = new Set<string>();
  raw.forEach((entry, index) => {
    const path = `probes[${index}]`;
    const probe = asRecord(entry);
    if (!probe) {
      sink.push("invalid_probe", "a probe must be an object", path);
      return;
    }

    if (!isNonEmptyString(probe.id)) {
      sink.push("invalid_probe_id", "a probe `id` must be a non-empty string", `${path}.id`);
    } else if (seenIds.has(probe.id)) {
      sink.push("duplicate_probe_id", `duplicate probe id "${probe.id}"`, `${path}.id`);
    } else {
      seenIds.add(probe.id);
    }

    if (!isProbeMethod(probe.method)) {
      sink.push(
        "invalid_method",
        `invalid probe method ${JSON.stringify(probe.method)}`,
        `${path}.method`,
      );
    }

    if (typeof probe.anchor !== "string") {
      sink.push("invalid_probe_anchor", "a probe `anchor` must be a string", `${path}.anchor`);
    } else if (!anchors.has(probe.anchor)) {
      sink.push(
        "probe_anchor_undeclared",
        `probe anchor "${probe.anchor}" is not declared in scenario.anchors`,
        `${path}.anchor`,
      );
    }

    if (!isMappingPolicy(probe.mappingPolicy)) {
      sink.push(
        "invalid_mapping_policy",
        `invalid mappingPolicy ${JSON.stringify(probe.mappingPolicy)}`,
        `${path}.mappingPolicy`,
      );
    }
    if (!isConfidence(probe.confidence)) {
      sink.push(
        "invalid_confidence",
        `invalid confidence ${JSON.stringify(probe.confidence)}`,
        `${path}.confidence`,
      );
    }
    if (!isSemanticDimension(probe.dimension)) {
      sink.push(
        "invalid_dimension",
        `invalid dimension ${JSON.stringify(probe.dimension)}`,
        `${path}.dimension`,
      );
    }
    if (typeof probe.requiresSourceMap !== "boolean") {
      sink.push(
        "invalid_requires_source_map",
        "`requiresSourceMap` must be a boolean",
        `${path}.requiresSourceMap`,
      );
    }

    if (!Array.isArray(probe.requiredDrivers)) {
      sink.push(
        "invalid_required_drivers",
        "`requiredDrivers` must be an array",
        `${path}.requiredDrivers`,
      );
    } else {
      probe.requiredDrivers.forEach((driver, di) => {
        if (!isRequiredDriver(driver)) {
          sink.push(
            "invalid_required_driver",
            `invalid requiredDriver ${JSON.stringify(driver)}`,
            `${path}.requiredDrivers[${di}]`,
          );
        }
      });
    }

    if (!Array.isArray(probe.capabilityRequirements)) {
      sink.push(
        "invalid_capability_requirements",
        "`capabilityRequirements` must be an array",
        `${path}.capabilityRequirements`,
      );
    } else {
      // The capability union is EXTENSIBLE: an unknown non-empty string is
      // admitted; only a non-string/empty entry is a fault.
      probe.capabilityRequirements.forEach((cap, ci) => {
        if (!isNonEmptyString(cap)) {
          sink.push(
            "invalid_capability_requirement",
            "a capabilityRequirement must be a non-empty string",
            `${path}.capabilityRequirements[${ci}]`,
          );
        }
      });
    }

    // Cross-field rules — each runs only when its inputs are valid enums/types.
    if (isMappingPolicy(probe.mappingPolicy) && typeof probe.requiresSourceMap === "boolean") {
      const needsMap = mappingPolicyRequiresSourceMap(probe.mappingPolicy);
      if (probe.requiresSourceMap && !needsMap) {
        sink.push(
          "source_map_required_but_mapping_policy_none",
          "a `none` mapping-policy probe is a direct Vue-surface probe and must not require a source map",
          `${path}.requiresSourceMap`,
        );
      } else if (!probe.requiresSourceMap && needsMap) {
        sink.push(
          "mapping_policy_requires_source_map",
          `mappingPolicy "${probe.mappingPolicy}" maps through the emitted artifact and requires a source map`,
          `${path}.requiresSourceMap`,
        );
      }
    }

    if (isProbeMethod(probe.method) && isSemanticDimension(probe.dimension)) {
      if (!methodSupportsDimension(probe.method, probe.dimension)) {
        sink.push(
          "method_dimension_structurally_impossible",
          `method "${probe.method}" cannot carry dimension "${probe.dimension}" — an operational signal has no Vue-surface semantic assertion`,
          `${path}.dimension`,
        );
      }
    }

    if (isConfidence(probe.confidence) && isMappingPolicy(probe.mappingPolicy)) {
      if (!confidenceWithinCeiling(probe.confidence, probe.mappingPolicy)) {
        sink.push(
          "confidence_exceeds_mapping_policy_ceiling",
          `confidence "${probe.confidence}" exceeds the ceiling mappingPolicy "${probe.mappingPolicy}" structurally permits`,
          `${path}.confidence`,
        );
      }
    }
  });
}

function validateEditSteps(
  raw: unknown,
  field: "setup" | "script",
  anchors: ReadonlySet<string>,
  sink: ErrorSink,
): void {
  if (!Array.isArray(raw)) {
    sink.push(
      field === "setup" ? "invalid_setup" : "invalid_script",
      `\`${field}\` must be an array`,
      field,
    );
    return;
  }
  raw.forEach((entry, index) => {
    const path = `${field}[${index}]`;
    const step = asRecord(entry);
    if (!step) {
      sink.push("invalid_edit_step", "an edit step must be an object", path);
      return;
    }
    if (!isEditStepKind(step.kind)) {
      sink.push(
        "invalid_edit_step_kind",
        `invalid edit-step kind ${JSON.stringify(step.kind)}`,
        `${path}.kind`,
      );
    }
    if (typeof step.anchor !== "string") {
      sink.push(
        "invalid_edit_step_anchor",
        "an edit-step `anchor` must be a string",
        `${path}.anchor`,
      );
    } else if (!anchors.has(step.anchor)) {
      sink.push(
        "edit_step_anchor_undeclared",
        `edit-step anchor "${step.anchor}" is not declared in scenario.anchors`,
        `${path}.anchor`,
      );
    }
    if ((step.kind === "insert" || step.kind === "replace") && typeof step.text !== "string") {
      sink.push(
        "invalid_edit_step_text",
        `a "${step.kind}" step requires a string \`text\``,
        `${path}.text`,
      );
    }
    if (step.kind === "replace" || step.kind === "delete") {
      if (
        typeof step.removeUnits !== "number" ||
        !Number.isFinite(step.removeUnits) ||
        step.removeUnits < 0
      ) {
        sink.push(
          "invalid_edit_step_remove_units",
          `a "${step.kind}" step requires a non-negative \`removeUnits\``,
          `${path}.removeUnits`,
        );
      }
    }
    // Only an insert/replace can burst (per-character typing); a delete is atomic.
    if (step.kind === "delete" && step.burst === true) {
      sink.push("burst_unsupported_for_step_kind", "a `delete` step cannot burst", `${path}.burst`);
    }
  });
}

function validateInvariants(raw: unknown, anchors: ReadonlySet<string>, sink: ErrorSink): void {
  if (!Array.isArray(raw)) {
    sink.push("invalid_invariants", "`invariants` must be an array", "invariants");
    return;
  }
  const seenIds = new Set<string>();
  raw.forEach((entry, index) => {
    const path = `invariants[${index}]`;
    const inv = asRecord(entry);
    if (!inv) {
      sink.push("invalid_invariant", "an invariant must be an object", path);
      return;
    }
    if (!isNonEmptyString(inv.id)) {
      sink.push(
        "invalid_invariant_id",
        "an invariant `id` must be a non-empty string",
        `${path}.id`,
      );
    } else if (seenIds.has(inv.id)) {
      sink.push("duplicate_invariant_id", `duplicate invariant id "${inv.id}"`, `${path}.id`);
    } else {
      seenIds.add(inv.id);
    }
    if (typeof inv.anchor !== "string") {
      sink.push(
        "invalid_invariant_anchor",
        "an invariant `anchor` must be a string",
        `${path}.anchor`,
      );
    } else if (!anchors.has(inv.anchor)) {
      sink.push(
        "invariant_anchor_undeclared",
        `invariant anchor "${inv.anchor}" is not declared in scenario.anchors`,
        `${path}.anchor`,
      );
    }
    if (!isProbeMethod(inv.method)) {
      sink.push(
        "invalid_invariant_method",
        `invalid invariant method ${JSON.stringify(inv.method)}`,
        `${path}.method`,
      );
    } else if (!methodSupportsInvariant(inv.method)) {
      sink.push(
        "invariant_method_not_a_surface",
        `invariant method "${inv.method}" is an operational signal with no Vue surface to assert on`,
        `${path}.method`,
      );
    }
    if (!isInvariantAssertion(inv.assertion)) {
      sink.push(
        "invalid_invariant_assertion",
        `invalid invariant assertion ${JSON.stringify(inv.assertion)}`,
        `${path}.assertion`,
      );
    }
    if (typeof inv.value !== "string") {
      sink.push(
        "invalid_invariant_value",
        "an invariant `value` must be a string",
        `${path}.value`,
      );
    }
  });
}

function validateBaselines(raw: unknown, sink: ErrorSink): void {
  const baselines = asRecord(raw);
  if (!baselines) {
    sink.push("invalid_baselines", "`baselines` must be an object", "baselines");
    return;
  }
  for (const provider of ["tsgo", "tsserver", "volar"] as const) {
    if (!isBaselineRequirement(baselines[provider])) {
      sink.push(
        "invalid_baseline_requirement",
        `invalid baseline requirement for ${provider}: ${JSON.stringify(baselines[provider])}`,
        `baselines.${provider}`,
      );
    }
  }
}

function validateNumber(value: unknown, path: string, sink: ErrorSink): void {
  if (value === undefined) return;
  if (typeof value !== "number" || !Number.isFinite(value)) {
    sink.push("invalid_threshold", `\`${path}\` must be a finite number`, path);
  } else if (value < 0) {
    sink.push("negative_threshold", `\`${path}\` must not be negative`, path);
  }
}

/**
 * Validate an OPTIONAL nested threshold sub-object (`latency` / `recovery`). An
 * absent value is fine; a PRESENT but non-object value (a number, a string, an
 * array, `null`) is a fault — never a silent skip — and only a true object reaches
 * `validateFields` for its declared numeric members.
 */
function validateThresholdObject(
  value: unknown,
  path: string,
  validateFields: (record: Record<string, unknown>) => void,
  sink: ErrorSink,
): void {
  if (value === undefined) return;
  const record = asRecord(value);
  if (!record) {
    sink.push("invalid_threshold_field", `\`${path}\` must be an object when present`, path);
    return;
  }
  validateFields(record);
}

function validateThresholds(raw: unknown, sink: ErrorSink): void {
  const thresholds = asRecord(raw);
  if (!thresholds) {
    // `thresholds` is a required part of the model: a passing validation must
    // imply its presence, so an absent or non-object value is a fault here
    // rather than a silently-skipped section.
    sink.push("invalid_thresholds", "`thresholds` is required and must be an object", "thresholds");
    return;
  }
  // Every field `ScenarioThresholds` declares is validated when present: a
  // present-but-malformed nested object is `invalid_threshold_field`, and each
  // declared numeric (sub-)field is range-checked. A non-object `latency`/`recovery`
  // is no longer silently accepted just because `asRecord` returns undefined.
  validateThresholdObject(
    thresholds.latency,
    "thresholds.latency",
    (latency) => {
      validateNumber(latency.p50Ms, "thresholds.latency.p50Ms", sink);
      validateNumber(latency.p95Ms, "thresholds.latency.p95Ms", sink);
      validateNumber(latency.p99Ms, "thresholds.latency.p99Ms", sink);
    },
    sink,
  );
  validateNumber(thresholds.steadyStateCompileDelta, "thresholds.steadyStateCompileDelta", sink);
  validateThresholdObject(
    thresholds.recovery,
    "thresholds.recovery",
    (recovery) => {
      validateNumber(recovery.maxRecoveryMs, "thresholds.recovery.maxRecoveryMs", sink);
      validateNumber(recovery.stableIntervals, "thresholds.recovery.stableIntervals", sink);
    },
    sink,
  );
  validateNumber(thresholds.flakeWindows, "thresholds.flakeWindows", sink);
}

/**
 * Validate a candidate scenario authored as data. Total over any `unknown` input;
 * returns `{ ok, errors }` where each error carries a discriminable {@link ScenarioErrorCode}.
 */
export function validateScenario(input: unknown): ScenarioValidationResult {
  const sink = new ErrorSink();
  const scenario = asRecord(input);
  if (!scenario) {
    sink.push("scenario_not_an_object", "a scenario must be an object", "");
    return { ok: false, errors: sink.errors };
  }

  if (!isNonEmptyString(scenario.id)) {
    sink.push("invalid_scenario_id", "`id` must be a non-empty string", "id");
  }
  if (!isNonEmptyString(scenario.fixture)) {
    sink.push("invalid_fixture", "`fixture` must be a non-empty string", "fixture");
  }
  if (!isNonEmptyString(scenario.entryFile)) {
    sink.push("invalid_entry_file", "`entryFile` must be a non-empty string", "entryFile");
  }

  // The declared anchor name set — referenced by probes, edit steps, and invariants.
  const anchorSet = new Set<string>();
  if (!Array.isArray(scenario.anchors)) {
    sink.push("invalid_anchors", "`anchors` must be an array of names", "anchors");
  } else {
    scenario.anchors.forEach((name, index) => {
      if (!isNonEmptyString(name)) {
        sink.push(
          "invalid_anchors",
          "an anchor name must be a non-empty string",
          `anchors[${index}]`,
        );
        return;
      }
      if (anchorSet.has(name)) {
        sink.push(
          "duplicate_anchor_declaration",
          `duplicate anchor declaration "${name}"`,
          `anchors[${index}]`,
        );
      }
      anchorSet.add(name);
    });
  }

  if (scenario.setup !== undefined) validateEditSteps(scenario.setup, "setup", anchorSet, sink);
  validateEditSteps(scenario.script, "script", anchorSet, sink);
  validateProbes(scenario.probes, anchorSet, sink);
  validateInvariants(scenario.invariants, anchorSet, sink);
  validateBaselines(scenario.baselines, sink);
  validateThresholds(scenario.thresholds, sink);

  return { ok: sink.errors.length === 0, errors: sink.errors };
}
