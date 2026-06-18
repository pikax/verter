import { describe, expect, it } from "vitest";

import {
  findCompletionItem,
  verifyAutoImport,
  type ExpectedImport,
} from "../src/collectors/index.js";
import type { CollectorEventKey } from "../src/collectors/index.js";
import type { CanonicalCompletionItem } from "../src/index.js";

/**
 * VERIFIER-UNIT tests (NOT a real-provider differential): these exercise the
 * pure verification layer — `verifyAutoImport` / `findCompletionItem` — over
 * HAND-BUILT resolved items. They prove the VERIFIER classifies a correct
 * resolved item as `applied` and a no-edit one as a divergence; they do NOT
 * spawn a provider, so they do not by themselves prove tsserver resolves an
 * import edit.
 *
 * The REAL provider-parity proof — a live tsgo AND tsserver each resolving the
 * SAME `additionalTextEdits` for an unimported symbol — lives in
 * `providerResolveParity.integration.test.ts` (gated on `DX_BASELINE_BIN`,
 * require-mode via `DX_REQUIRE_PROVIDERS=1`). These unit tests are the
 * fast-feedback companion to that integration gate: they pin the verifier's
 * semantics so a verifier regression is caught without a provider spawn.
 */

const before = "import { ref } from 'vue'\nconst a = 1\n";

const expectedImport: ExpectedImport = { symbol: "computed", module: "vue" };

function keyFor(provider: string): CollectorEventKey {
  return {
    scenario: "auto-import",
    editStepIndex: 0,
    driver: "rawLsp",
    provider,
    probe: "auto-import-computed",
    version: 2,
    anchor: "cursor",
  };
}

/** The auto-import edit a correct resolve produces for `computed` from `vue`. */
function resolvedComputedItem(): CanonicalCompletionItem {
  return {
    label: "computed",
    additionalTextEdits: [
      {
        range: { start: { line: 1, character: 0 }, end: { line: 1, character: 0 } },
        newText: "import { computed } from 'vue'\n",
      },
    ],
  };
}

/** The classification verifyAutoImport reaches for one provider's resolved item. */
function outcomeSignal(provider: string, item: CanonicalCompletionItem): string {
  const events = verifyAutoImport({ key: keyFor(provider), before, item, expectedImport });
  const applied = events.find((e) => e.signal === "auto_import_applied");
  if (applied?.ok === true) return "applied";
  const failed = events.find((e) => !e.ok);
  return failed?.signal ?? "none";
}

describe("auto-import provider parity — both providers agree on the bound import", () => {
  it("tsgo and tsserver resolves of the same entry produce the SAME applied import", () => {
    // The provider-neutral contract: tsserver now resolves auto-import edits just
    // like tsgo. Both hand back the same import edit → both verify as applied.
    const tsgo = outcomeSignal("tsgo", resolvedComputedItem());
    const tsserver = outcomeSignal("tsserver", resolvedComputedItem());
    expect(tsgo).toBe("applied");
    expect(tsserver).toBe("applied");
    expect(tsgo).toBe(tsserver);
  });

  it("the extension provider agrees with tsgo (all three are first-class)", () => {
    const tsgo = outcomeSignal("tsgo", resolvedComputedItem());
    const extension = outcomeSignal("extension", resolvedComputedItem());
    expect(extension).toBe("applied");
    expect(extension).toBe(tsgo);
  });

  it("DIVERGENCE: a provider that returns no auto-import edit (the pre-fix tsserver) disagrees", () => {
    // This is the EXACT pre-fix tsserver behavior: resolve returns nothing, so no
    // `additionalTextEdits`. The parity assertion catches it — tsgo applies, the
    // broken provider yields `auto_import_empty_edit`. Discriminating: the
    // outcomes MUST differ here, proving the parity test would fail a regression.
    const working = outcomeSignal("tsgo", resolvedComputedItem());
    const brokenItem: CanonicalCompletionItem = { label: "computed" }; // no edits
    const broken = outcomeSignal("tsserver", brokenItem);
    expect(working).toBe("applied");
    expect(broken).toBe("auto_import_empty_edit");
    expect(working).not.toBe(broken);
  });
});

describe("findCompletionItem — locates the resolve-bearing item by label across providers", () => {
  it("finds an item carrying the provider-neutral verter_resolve envelope as its data", () => {
    // After the fix, the raw item's `data` is the provider-neutral envelope
    // (NOT the old `{ tsgo: true, original_data }` shape). `findCompletionItem`
    // is data-shape-agnostic — it locates by label so the resolve round-trip
    // works regardless of which provider stamped the envelope.
    const raw = {
      items: [
        { label: "ref", data: null },
        {
          label: "computed",
          data: {
            verter_resolve: {
              kind: "type_provider",
              provider_id: "tsserver",
              provider_path: "/ws/App.vue.tsx",
              provider_data: { kind: "tsserver_entry", name: "computed", offset: 7 },
            },
          },
        },
      ],
    };
    const found = findCompletionItem(raw as never, "computed") as
      | { label?: string; data?: { verter_resolve?: { provider_id?: string } } }
      | undefined;
    expect(found?.label).toBe("computed");
    // The resolve envelope rides on the found item's data, ready to send back to
    // the originating provider for `completionItem/resolve`.
    expect(found?.data?.verter_resolve?.provider_id).toBe("tsserver");
  });

  it("returns undefined for a label not offered (no candidate to resolve)", () => {
    const raw = { items: [{ label: "ref" }] };
    expect(findCompletionItem(raw as never, "computed")).toBeUndefined();
  });
});
