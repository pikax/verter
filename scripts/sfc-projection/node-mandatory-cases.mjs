/**
 * The canonical per-node mandatory-case table shared by the projection probe
 * harness and every node protocol that must derive required rows from it
 * (STP8 ratification) instead of restating its own list. Kept in a module of
 * its own so a protocol can import it without a circular import back into the
 * CLI entry point (verify-node.mjs evaluates under a top-level await).
 */
export const NODE_MANDATORY_CASES = Object.freeze({
  STP1: Object.freeze([
    "STP1-inventory",
    "STP1-zero-selection",
    "STP1-clean-twin",
    "STP1-types",
    "STP1-provenance",
    "STP1-harness",
  ]),
  STP2: Object.freeze([
    "STP2-instance-concrete",
    "STP2-instance-generic",
    "STP2-instance-explicit",
    "STP2-constructor-escape",
    "STP2-vue-utilities",
    "STP2-not-callable",
    "STP2-constructor-inferred",
    "STP2-explicit-input-mismatch",
  ]),
  STP3: Object.freeze([
    "STP3-coupled",
    "STP3-wrong-channel",
    "STP3-inference-only-channel",
    "STP3-order-independent",
    "STP3-ordered-merge",
    "STP3-fresh-uses",
  ]),
  STP4: Object.freeze([
    "STP4-js-unchecked",
    "STP4-js-checked",
    "STP4-tsx-authored",
    "STP4-supplemental-import",
    "STP4-external-owner",
    "STP4-illegal-vue",
  ]),
  STP5: Object.freeze([
    "STP5-encoding",
    "STP5-guard-duplicate",
    "STP5-alias-edit",
    "STP5-stale-target",
    "STP5-raw-cli",
    "STP5-capability",
  ]),
  STP6: Object.freeze([
    "STP6-package-instance",
    "STP6-package-generics",
    "STP6-hidden-metadata",
    "STP6-closure",
    "STP6-decl-map",
    "STP6-resolution",
  ]),
  STP7: Object.freeze([
    "STP7-svelte-shape",
    "STP7-holes",
    "STP7-realm",
    "STP7-reuse",
    "STP7-scope-claim",
  ]),
  STP8: Object.freeze([
    "STP8-complete-evidence",
    "STP8-partial-ratify",
    "STP8-abi-contamination",
    "STP8-inference-contract",
  ]),
  STS0: Object.freeze([
    "STS0-svelte-inventory",
    "STS0-svelte-abi",
    "STS0-svelte-pin",
    "STS0-policy-lock",
  ]),
});

export const MANDATORY_CASES = NODE_MANDATORY_CASES.STP1;

export const STP3_MANDATORY_CASES = NODE_MANDATORY_CASES.STP3;
export const STP4_MANDATORY_CASES = NODE_MANDATORY_CASES.STP4;
export const STP5_MANDATORY_CASES = NODE_MANDATORY_CASES.STP5;
export const STP6_MANDATORY_CASES = NODE_MANDATORY_CASES.STP6;
export const STP7_MANDATORY_CASES = NODE_MANDATORY_CASES.STP7;
export const STP8_MANDATORY_CASES = NODE_MANDATORY_CASES.STP8;
export const STS0_MANDATORY_CASES = NODE_MANDATORY_CASES.STS0;
