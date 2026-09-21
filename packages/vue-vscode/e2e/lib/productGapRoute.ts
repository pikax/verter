import {
  knownFrameworkContractGapsForRoute,
  type ContractFramework,
} from "./frameworkContractManifest";
import {
  knownProductGapCanariesForRoute,
  knownProductGapsForRoute,
  type ProductGapManifest,
} from "./knownProductGapManifest";

const CONTRACT_FRAMEWORK_BY_FIXTURE: Readonly<Record<string, ContractFramework>> = {
  "vue-contract": "vue",
  "svelte-contract": "svelte",
};

/** Exact known product gaps whose test bodies must not run on this fixture/provider route. */
export function productGapsForFixtureRoute(
  fixture: string,
  typeProvider: string | undefined,
): ProductGapManifest {
  const framework = CONTRACT_FRAMEWORK_BY_FIXTURE[fixture];
  if (framework) return knownFrameworkContractGapsForRoute(framework, typeProvider);
  if (!fixture.endsWith("-parity") || !typeProvider) return {};
  return knownProductGapsForRoute(fixture, typeProvider);
}

/** Exact canaries whose test bodies run, and are expected to fail, on this fixture/provider route. */
export function productGapCanariesForFixtureRoute(
  fixture: string,
  typeProvider: string | undefined,
): ProductGapManifest {
  if (!fixture.endsWith("-parity") || !typeProvider) return {};
  return knownProductGapCanariesForRoute(fixture, typeProvider);
}
