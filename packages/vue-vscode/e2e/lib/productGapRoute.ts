import {
  knownFrameworkContractGapsForRoute,
  type ContractFramework,
} from "./frameworkContractManifest";
import { knownProductGapsForRoute, type ProductGapManifest } from "./knownProductGapManifest";

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
