// Self-test: official-case coverage accounting (BF2 required exit — "every
// seed manifest declaration is either runner-enumerated or has a reviewed
// allowed disposition").

import { describe, expect, it } from "vitest";

import {
  accountManifestStructure,
  parseCaseManifest,
  reEnumerateVueRows,
  reEnumerateSvelteRows,
} from "../src/coverage-report.mjs";
import { oracleSourcePaths } from "../src/env-paths.mjs";

describe("manifest structural accounting", () => {
  it("Vue manifest: exactly 2003 rows, unique IDs, closed-set dispositions, no unexplained row", () => {
    const result = accountManifestStructure("vue-official-cases.tsv");
    expect(result.rowCount).toBe(2003);
    expect(result.uniqueIds).toBe(2003);
    expect(result.problems).toEqual([]);
  });

  it("Svelte manifest: exactly 3457 rows, unique IDs, closed-set dispositions, no unexplained row", () => {
    const result = accountManifestStructure("svelte-official-cases.tsv");
    expect(result.rowCount).toBe(3457);
    expect(result.uniqueIds).toBe(3457);
    expect(result.problems).toEqual([]);
  });
});

describe("runner re-enumeration against the pinned source trees", () => {
  const { vueSource, svelteSource } = oracleSourcePaths();
  const runIf = vueSource && svelteSource ? it : it.skip;

  runIf("every one of the 2003 Vue rows resolves inside the pinned checkout", () => {
    const rows = parseCaseManifest("vue-official-cases.tsv");
    const result = reEnumerateVueRows(vueSource, rows);
    expect(result.unresolvable).toEqual([]);
    expect(result.resolvable).toBe(2003);
  });

  runIf("every one of the 3457 Svelte rows resolves inside the pinned checkout", () => {
    const rows = parseCaseManifest("svelte-official-cases.tsv");
    const result = reEnumerateSvelteRows(svelteSource, rows);
    expect(result.unresolvable).toEqual([]);
    expect(result.resolvable).toBe(3457);
  });

  runIf(
    "a deliberately corrupted locator is correctly reported unresolvable (not silently accepted)",
    () => {
      const rows = [
        {
          case_id: "BF2-SELFTEST-BOGUS",
          source_locator: "packages/does-not-exist/__tests__/bogus.spec.ts:1:1",
        },
      ];
      const result = reEnumerateVueRows(vueSource, rows);
      expect(result.resolvable).toBe(0);
      expect(result.unresolvable).toEqual(["BF2-SELFTEST-BOGUS"]);
    },
  );
});
