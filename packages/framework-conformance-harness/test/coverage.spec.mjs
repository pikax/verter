// Self-test: official-case coverage accounting (BF2 required exit — "every
// seed manifest declaration is either runner-enumerated or has a reviewed
// allowed disposition").

import { describe, expect, it } from "vitest";

import {
  accountManifestStructure,
  parseCaseManifest,
  reEnumerateVueRows,
  reEnumerateSvelteRows,
  reverseEnumerateSvelteRows,
} from "../src/coverage-report.mjs";
import { oracleSourcePaths } from "../src/env-paths.mjs";

describe("manifest structural accounting", () => {
  it("Vue manifest: exactly 2003 rows, unique IDs, closed-set dispositions, no unexplained row", () => {
    const result = accountManifestStructure("vue-official-cases.tsv");
    expect(result.rowCount).toBe(2003);
    expect(result.uniqueIds).toBe(2003);
    expect(result.problems).toEqual([]);
  });

  it("Svelte manifest: exactly 3475 rows, unique IDs, closed-set dispositions, no unexplained row", () => {
    const result = accountManifestStructure("svelte-official-cases.tsv");
    expect(result.rowCount).toBe(3475);
    expect(result.uniqueIds).toBe(3475);
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

  runIf("every one of the 3475 Svelte rows resolves inside the pinned checkout", () => {
    const rows = parseCaseManifest("svelte-official-cases.tsv");
    const result = reEnumerateSvelteRows(svelteSource, rows);
    expect(result.unresolvable).toEqual([]);
    expect(result.resolvable).toBe(3475);
  });

  // The MISSING reverse direction: every real Svelte suite/sample directory
  // in the pinned checkout must have a covered row — not just "every
  // committed row still resolves" (the case above). Without this direction,
  // an upstream case silently added (or a case silently dropped from the
  // manifest by a bad hand-edit) goes undetected forever, which is exactly
  // how the pre-bump manifest rotted against 5.56.10 unnoticed.
  runIf(
    "bidirectional completeness: every upstream Svelte sample/suite locator has a covered row",
    () => {
      const rows = parseCaseManifest("svelte-official-cases.tsv");
      const result = reverseEnumerateSvelteRows(svelteSource, rows);
      expect(result.missingFromManifest).toEqual([]);
      expect(result.goneFromUpstream).toEqual([]);
      expect(result.liveTotal).toBe(3475);
    },
    // svelteManifest() spawns one `git rev-parse` per sample/suite directory
    // (~3475 of them, unbatched — it is the generator's own enumeration
    // logic, reused as-is rather than forked into a faster second walker) —
    // comfortably over vitest's default 5s test timeout.
    60000,
  );

  // Proves the reverse check actually discriminates: withholding a single
  // real, currently-covered row must be caught as "present upstream but
  // missing from the manifest" — not silently pass.
  runIf(
    "bidirectional completeness: a deliberately withheld real row is caught as missing",
    () => {
      const rows = parseCaseManifest("svelte-official-cases.tsv");
      const [withheldRow, ...remainder] = rows;
      const result = reverseEnumerateSvelteRows(svelteSource, remainder);
      expect(result.missingFromManifest).toEqual([withheldRow.source_locator]);
    },
    60000,
  );

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

  // Targets the content-hash discriminator specifically: a row whose PATH is
  // real and tracked in the pinned checkout (so it passes the path lookup the
  // "bogus locator" case above exercises) but whose recorded source_object
  // (git blob hash) has been deliberately corrupted to not match the live
  // blob at that path. Without this case, deleting the
  // `source_object-mismatch` branch in reEnumerateVueRows/reEnumerateSvelteRows
  // would go undetected by this suite even though the branch is live
  // production logic.
  runIf(
    "a row whose path resolves but whose recorded content hash has drifted is correctly reported unresolvable",
    () => {
      const [realRow] = parseCaseManifest("vue-official-cases.tsv");
      const corrupted = {
        ...realRow,
        case_id: "BF2-SELFTEST-HASH-MISMATCH",
        source_object: "0000000000000000000000000000000000000000",
      };
      const result = reEnumerateVueRows(vueSource, [corrupted]);
      expect(result.resolvable).toBe(0);
      expect(result.unresolvable).toEqual(["BF2-SELFTEST-HASH-MISMATCH"]);
    },
  );

  runIf(
    "a Svelte row whose path resolves but whose recorded content hash has drifted is correctly reported unresolvable",
    () => {
      const [realRow] = parseCaseManifest("svelte-official-cases.tsv");
      const corrupted = {
        ...realRow,
        case_id: "BF2-SELFTEST-SVELTE-HASH-MISMATCH",
        source_object: "0000000000000000000000000000000000000000",
      };
      const result = reEnumerateSvelteRows(svelteSource, [corrupted]);
      expect(result.resolvable).toBe(0);
      expect(result.unresolvable).toEqual(["BF2-SELFTEST-SVELTE-HASH-MISMATCH"]);
    },
  );
});
