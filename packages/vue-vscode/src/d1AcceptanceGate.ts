/**
 * The D1 editor-attach acceptance gate (pure, `vscode`-free).
 *
 * D1 is an env-gated real-editor acceptance. Its gate is HONEST: it is a hard gate
 * when REQUESTED, never a skip-pass that launders a missing prerequisite.
 *
 *   - `VERTER_E2E_D1` unset ⇒ the suite is NOT APPLICABLE (`skip`) — excluded from
 *     the default e2e matrix and the canonical Rust run.
 *   - `VERTER_E2E_D1` set but a prerequisite is missing (no resolvable tsgo, no
 *     built relay shim) ⇒ HARD `fail` — a requested gate whose environment cannot
 *     honestly satisfy it is a failure, NEVER a skip.
 *   - `VERTER_E2E_D1` set and every prerequisite present ⇒ `run` the assertions.
 *
 * The decision is a pure function of the observed environment so it is unit-testable
 * and discriminating (requested-but-missing ⇒ fail, requested-and-present ⇒ run,
 * unset ⇒ skip). The e2e `suiteSetup` maps `skip`→`this.skip()`, `fail`→`throw`.
 */

/** The env flag that REQUESTS the D1 acceptance gate (its presence makes it a hard gate). */
export const D1_GATE_ENV = "VERTER_E2E_D1";

/** The observed prerequisites the D1 gate decision keys on. */
export interface D1GateInputs {
  /** Truthy when `VERTER_E2E_D1` requests the gate. */
  requested: boolean;
  /** Whether a native-preview tsgo engine is resolvable (provisioning honored). */
  tsgoResolvable: boolean;
  /** Whether the built `verter-relay-shim` binary is present. */
  shimPresent: boolean;
}

export type D1GateDecision =
  | { action: "skip"; reason: string }
  | { action: "fail"; reason: string }
  | { action: "run" };

/**
 * Decide the D1 gate action from the observed prerequisites. HONEST-GATE contract:
 * a REQUESTED gate (flag set) with any missing prerequisite is `fail`, never `skip`.
 */
export function evaluateD1Gate(inputs: D1GateInputs): D1GateDecision {
  if (!inputs.requested) {
    return {
      action: "skip",
      reason: `${D1_GATE_ENV} is not set — D1 editor-attach acceptance is not applicable`,
    };
  }
  // Requested ⇒ every prerequisite MUST be present, else HARD FAIL (never skip-pass).
  if (!inputs.tsgoResolvable) {
    return {
      action: "fail",
      reason: `${D1_GATE_ENV} is set but no native-preview tsgo engine is resolvable — a requested D1 gate with a missing engine is a FAILURE, not a skip`,
    };
  }
  if (!inputs.shimPresent) {
    return {
      action: "fail",
      reason: `${D1_GATE_ENV} is set but the verter-relay-shim binary is not built — a requested D1 gate with a missing shim is a FAILURE, not a skip`,
    };
  }
  return { action: "run" };
}

/** Read the D1 gate request flag from an environment map. */
export function d1GateRequested(env: Record<string, string | undefined>): boolean {
  const value = env[D1_GATE_ENV];
  return value !== undefined && value !== "" && value !== "0" && value !== "false";
}
