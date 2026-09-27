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
  STP9: Object.freeze([
    "STP9-ids",
    "STP9-shadow",
    "STP9-type-free",
    "STP9-complete-cache",
    "STP9-determinism",
  ]),
  STP10: Object.freeze([
    "STP10-roundtrip",
    "STP10-role",
    "STP10-overlap",
    "STP10-stale-map",
    "STP10-synthetic",
  ]),
  STP11: Object.freeze([
    "STP11-universal",
    "STP11-scope",
    "STP11-await",
    "STP11-assertion",
    "STP11-one-body",
  ]),
  STP12: Object.freeze([
    "STP12-checkjs-off",
    "STP12-checkjs-on",
    "STP12-jsdoc-generic",
    "STP12-jsx",
    "STP12-suppression",
  ]),
  STP13: Object.freeze([
    "STP13-options-this",
    "STP13-mixins",
    "STP13-combined",
    "STP13-shadowed-macro",
    "STP13-options-instance",
  ]),
  STP14: Object.freeze([
    "STP14-local-capture",
    "STP14-dependent-default",
    "STP14-typeof-capture",
    "STP14-alias-cycle",
    "STP14-duplicate-error",
  ]),
  STP15: Object.freeze([
    "STP15-read-ref",
    "STP15-readonly-write",
    "STP15-setter-domain",
    "STP15-unused",
    "STP15-mutation",
  ]),
  STP16: Object.freeze([
    "STP16-required-api",
    "STP16-public-members",
    "STP16-private-leak",
    "STP16-generic-constraint",
    "STP16-public-callable",
    "STP16-type-precision",
    "STP16-public-specialization",
  ]),
  STP17: Object.freeze([
    "STP17-spellings",
    "STP17-merge",
    "STP17-overwrite",
    "STP17-optional-spread",
    "STP17-collision",
  ]),
  STP18: Object.freeze([
    "STP18-single-witness",
    "STP18-uncoupled",
    "STP18-contextual",
    "STP18-literal",
    "STP18-handler-check",
    "STP18-fresh-id",
    "STP18-script-template-parity",
  ]),
  STP19: Object.freeze([
    "STP19-explicit",
    "STP19-higher-rank",
    "STP19-forward",
    "STP19-overloads",
    "STP19-foreign",
    "STP19-erasure",
    "STP19-instantiation-alias",
  ]),
  STP20: Object.freeze([
    "STP20-default",
    "STP20-required",
    "STP20-spread-extra",
    "STP20-union",
    "STP20-optional",
    "STP20-overwritten",
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
export const STP9_MANDATORY_CASES = NODE_MANDATORY_CASES.STP9;
export const STP10_MANDATORY_CASES = NODE_MANDATORY_CASES.STP10;
export const STP11_MANDATORY_CASES = NODE_MANDATORY_CASES.STP11;
export const STP12_MANDATORY_CASES = NODE_MANDATORY_CASES.STP12;
export const STP13_MANDATORY_CASES = NODE_MANDATORY_CASES.STP13;
export const STP14_MANDATORY_CASES = NODE_MANDATORY_CASES.STP14;
export const STP15_MANDATORY_CASES = NODE_MANDATORY_CASES.STP15;
export const STP16_MANDATORY_CASES = NODE_MANDATORY_CASES.STP16;
export const STP17_MANDATORY_CASES = NODE_MANDATORY_CASES.STP17;
export const STP18_MANDATORY_CASES = NODE_MANDATORY_CASES.STP18;
export const STP19_MANDATORY_CASES = NODE_MANDATORY_CASES.STP19;
export const STP20_MANDATORY_CASES = NODE_MANDATORY_CASES.STP20;
export const STS0_MANDATORY_CASES = NODE_MANDATORY_CASES.STS0;
