// Provider-aware CI partitioning for the one standard Rust nextest archive.
//
// Keep broad ownership structural: provider-specific modules and packages move
// as a unit. Exact names are reserved for real-engine tests embedded in mixed
// provider-neutral modules that cannot move without dragging the whole module
// into one provider lane.

import { buildCanonicalSurface1FilterExpr, TRYBUILD_EXCLUDED_SUITES } from "./gate-internals.mjs";

export const PROVIDER_CI_LANES = Object.freeze(["core", "tsserver", "tsgo"]);

export const PROVIDER_LIVE_SELECTORS = Object.freeze([
  {
    lane: "tsserver",
    package: "verter_lsp",
    kind: "regex",
    value: "^real_provider_tests::.*_tsserver$",
    example: "real_provider_tests::completion::completion_app_vue_template_tsserver",
    label: "paired LSP real-provider tsserver variants",
  },
  {
    lane: "tsserver",
    package: "verter_lsp",
    kind: "prefix",
    value: "tsserver::",
    example: "tsserver::project_router::tests::engine_identity_includes_project_and_install",
    label: "LSP tsserver implementation tests",
  },
  {
    lane: "tsserver",
    package: "verter_lsp",
    kind: "prefix",
    value: "cases::tsserver_e2e_generated_outputs::",
    example:
      "cases::tsserver_e2e_generated_outputs::test_e2e_tsserver_scoped_slot_types_from_generated_vue_outputs",
    label: "LSP generated-output tsserver integration",
  },
  {
    lane: "tsserver",
    package: "verter_lsp",
    kind: "exact",
    values: Object.freeze([
      "cases::quoted_prop_consumer_mistype_live::quoted_prop_consumer_mistype_surfaces_ts2322_tsserver",
      "server::server_tests::completion_with_real_tsserver_returns_fixture_vfor_member_access_properties",
      "server::server_tests::completion_with_real_tsserver_recovers_fixture_vfor_member_access_immediately_after_open",
      "server::server_tests::completion_with_real_tsserver_recovers_fixture_vfor_member_access_on_dot_trigger_immediately_after_open",
      "server::server_tests::completion_with_real_tsserver_recovers_when_current_file_sync_was_missed",
      "server::server_tests::real_tsserver_slot_member_access_stays_typed_after_opening_child_and_parent",
      "type_provider::lazy_managed_tests::failed_activation_replays_real_vue_and_svelte_carriers_before_typed_queries",
    ]),
    label: "real tsserver tests embedded in mixed LSP modules",
  },
  {
    lane: "tsserver",
    package: "verter_type_runtime",
    kind: "prefix",
    value: "tsserver::",
    example: "tsserver::ipc_tests::test_parse_tsserver_completion",
    label: "type-runtime tsserver implementation tests",
  },
  {
    lane: "tsserver",
    package: "verter_type_runtime",
    kind: "exact",
    values: Object.freeze([
      "resilient::resilient_tests::failed_respawn_retries_within_budget_and_recovers",
    ]),
    label: "real tsserver recovery test embedded in the resilient-provider module",
  },
  {
    lane: "tsgo",
    package: "verter_lsp",
    kind: "regex",
    value: "^real_provider_tests::.*_tsgo$",
    example: "real_provider_tests::completion::completion_app_vue_template_tsgo",
    label: "paired LSP real-provider tsgo variants",
  },
  {
    lane: "tsgo",
    package: "verter_lsp",
    kind: "prefix",
    value: "tsgo::",
    example: "tsgo::shared_tests::shared_attach_selection_is_provider_neutral",
    label: "LSP tsgo implementation tests",
  },
  {
    lane: "tsgo",
    package: "verter_lsp",
    kind: "prefix",
    value: "cases::shared_provider_live::",
    example: "cases::shared_provider_live::shared_provider_serves_real_vue_macro_carrier",
    label: "shared-tsgo LSP integration",
  },
  {
    lane: "tsgo",
    package: "verter_lsp",
    kind: "prefix",
    value: "cases::tsgo_virtual_membership::",
    example:
      "cases::tsgo_virtual_membership::vue_specific_include_companion_becomes_member_via_virtualization",
    label: "managed-tsgo virtual membership integration",
  },
  {
    lane: "tsgo",
    package: "verter_lsp",
    kind: "exact",
    values: Object.freeze([
      "cases::quoted_prop_consumer_mistype_live::quoted_prop_consumer_mistype_surfaces_ts2322_tsgo",
      "type_provider::lazy_managed_tests::lazy_real_tsgo_quick_info_matches_the_eager_file_lifecycle",
    ]),
    label: "real tsgo tests embedded in mixed LSP modules",
  },
  {
    lane: "tsgo",
    package: "verter_type_runtime",
    kind: "prefix",
    value: "tsgo::",
    example: "tsgo::ipc_tests::test_tsgo_spawn_and_initialize",
    label: "type-runtime tsgo implementation tests",
  },
  {
    lane: "tsgo",
    package: "verter_type_runtime",
    kind: "prefix",
    value: "cases::owned_provider_live::",
    example:
      "cases::owned_provider_live::owned_provider_diagnostics_via_api_and_feature_via_lsp_one_process",
    label: "owned tsgo provider integration",
  },
  {
    lane: "tsgo",
    package: "verter_type_runtime",
    kind: "prefix",
    value: "cases::owned_provider_carrier_resolution::",
    example:
      "cases::owned_provider_carrier_resolution::owned_bare_vue_import_resolves_to_declaration_carrier_and_public_member_flows",
    label: "owned tsgo carrier-resolution integration",
  },
  {
    lane: "tsgo",
    package: "verter_tsgo_api",
    kind: "package",
    example: "tests::toolchain_resolution_preserves_candidate_order",
    label: "tsgo API package",
  },
  {
    lane: "tsgo",
    package: "verter_relay_shim",
    kind: "package",
    example: "tests::classifies_forwarded_requests",
    label: "shared-tsgo relay package",
  },
  {
    lane: "tsgo",
    package: "verter_session",
    kind: "prefix",
    value: "cases::svelte_typecheck_gate::",
    example: "cases::svelte_typecheck_gate::projected_runes_props_fixture_type_checks_clean",
    label: "tsgo-backed Svelte projection typecheck gate",
  },
]);

function escapeNextestRegex(value) {
  return value.replace(/[\\^$.*+?()[\]{}|/]/g, "\\$&");
}

function selectorTestExpr(selector) {
  switch (selector.kind) {
    case "package":
      return null;
    case "prefix":
      return `test(/^${escapeNextestRegex(selector.value)}/)`;
    case "regex":
      return `test(/${selector.value}/)`;
    case "exact":
      return `(${selector.values.map((name) => `test(=${name})`).join(" or ")})`;
    default:
      throw new Error(`unknown provider CI selector kind: ${selector.kind}`);
  }
}

export function selectorFilterExpr(selector) {
  const testExpr = selectorTestExpr(selector);
  return testExpr
    ? `(package(${selector.package}) and ${testExpr})`
    : `package(${selector.package})`;
}

export function buildProviderLaneFilterExpr(lane) {
  if (!PROVIDER_CI_LANES.includes(lane)) {
    throw new Error(`unknown provider CI lane '${lane}'`);
  }
  const canonical = buildCanonicalSurface1FilterExpr();
  const allProvider = PROVIDER_LIVE_SELECTORS.map(selectorFilterExpr).join(" or ");
  if (lane === "core") return `(${canonical}) and not (${allProvider})`;
  const selected = PROVIDER_LIVE_SELECTORS.filter((selector) => selector.lane === lane)
    .map(selectorFilterExpr)
    .join(" or ");
  return `(${canonical}) and (${selected})`;
}

function selectorMatches(selector, packageName, testName) {
  if (selector.package !== packageName) return false;
  switch (selector.kind) {
    case "package":
      return true;
    case "prefix":
      return testName.startsWith(selector.value);
    case "regex":
      return new RegExp(selector.value).test(testName);
    case "exact":
      return selector.values.includes(testName);
    default:
      throw new Error(`unknown provider CI selector kind: ${selector.kind}`);
  }
}

function isCanonicalSurfaceTest(packageName, testName) {
  if (packageName === "verter_shipped_cfg_contract") return false;
  return !TRYBUILD_EXCLUDED_SUITES.some(
    (suite) => suite.package === packageName && testName.startsWith(suite.modulePrefix),
  );
}

export function verifyProviderCiPartition(listJson) {
  const counts = { core: 0, tsserver: 0, tsgo: 0 };
  const selectorCounts = PROVIDER_LIVE_SELECTORS.map(() => 0);
  const exactCounts = new Map();
  const errors = [];

  for (const selector of PROVIDER_LIVE_SELECTORS) {
    if (selector.kind !== "exact") continue;
    for (const name of selector.values) exactCounts.set(`${selector.package}\0${name}`, 0);
  }

  for (const suite of Object.values(listJson?.["rust-suites"] || {})) {
    const packageName = suite?.["package-name"];
    if (typeof packageName !== "string") {
      errors.push("nextest suite is missing package-name");
      continue;
    }
    for (const testName of Object.keys(suite.testcases || {})) {
      if (!isCanonicalSurfaceTest(packageName, testName)) continue;
      const matches = [];
      for (let index = 0; index < PROVIDER_LIVE_SELECTORS.length; index++) {
        const selector = PROVIDER_LIVE_SELECTORS[index];
        if (!selectorMatches(selector, packageName, testName)) continue;
        selectorCounts[index]++;
        matches.push(selector);
        const exactKey = `${packageName}\0${testName}`;
        if (exactCounts.has(exactKey)) exactCounts.set(exactKey, exactCounts.get(exactKey) + 1);
      }
      const lanes = [...new Set(matches.map((selector) => selector.lane))];
      if (matches.length > 1) {
        errors.push(
          `${packageName} ${testName} matches multiple provider selectors: ${matches
            .map((selector) => selector.label)
            .join(", ")}`,
        );
      }
      if (lanes.length > 1) {
        errors.push(`${packageName} ${testName} crosses provider lanes: ${lanes.join(", ")}`);
      }
      counts[lanes[0] || "core"]++;
    }
  }

  for (let index = 0; index < PROVIDER_LIVE_SELECTORS.length; index++) {
    if (selectorCounts[index] === 0) {
      errors.push(`provider selector matched zero tests: ${PROVIDER_LIVE_SELECTORS[index].label}`);
    }
  }
  for (const [key, count] of exactCounts) {
    if (count !== 1) {
      const [packageName, testName] = key.split("\0");
      errors.push(
        `exact provider test ${packageName} ${testName} matched ${count} times, expected 1`,
      );
    }
  }
  for (const lane of PROVIDER_CI_LANES) {
    if (counts[lane] === 0) errors.push(`provider CI lane '${lane}' selected zero tests`);
  }

  return { ok: errors.length === 0, counts, selectorCounts, errors };
}
