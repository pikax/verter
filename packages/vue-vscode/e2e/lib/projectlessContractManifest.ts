export const PROJECTLESS_CONTRACT_FIXTURES = ["no-config", "single-file"] as const;

export type ProjectlessContractFixture = (typeof PROJECTLESS_CONTRACT_FIXTURES)[number];

export const PROJECTLESS_CONTRACT_SUITE_GLOB = "projectless-contract.test";
export const PROJECTLESS_CONTRACT_LOADED_FILES = ["projectless-contract.test.js"] as const;

/** Exact row inventory for provider-off projectless acceptance. */
export const PROJECTLESS_CONTRACT_TEST_IDS = [
  "projectless.extension-active",
  "projectless.lsp-ready",
  "projectless.provider-none",
  "projectless.no-server-crash",
] as const;

export function isProjectlessContractFixture(
  fixture: string,
): fixture is ProjectlessContractFixture {
  return PROJECTLESS_CONTRACT_FIXTURES.includes(fixture as ProjectlessContractFixture);
}
