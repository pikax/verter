import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";

import { describe, expect, it } from "vitest";

import { ORACLE_FAMILIES, type OracleFamily } from "../src/semantic-oracle/model.js";
import { prepareOracleSource } from "../src/semantic-oracle/prepare.js";

/** Each oracle family's `.ts` file, its anchors → the identifier each must resolve to,
 *  and intended-semantics tokens that must be present in the (hermetic) source. */
const ORACLES: Record<
  OracleFamily,
  { file: string; anchors: Record<string, string>; tokens: readonly string[] }
> = {
  defineProps: {
    file: "define-props.ts",
    anchors: { "props.title": "title", "props.count": "count" },
    tokens: ["DrawerProps", "title: string", "count?: number"],
  },
  defineEmits: {
    file: "define-emits.ts",
    anchors: { emit: "emit" },
    tokens: ['"submit"', "value: string", '"close"'],
  },
  defineModel: {
    file: "define-model.ts",
    anchors: { "model.value": "value" },
    tokens: ["ModelRef", "boolean"],
  },
  slots: {
    file: "slots.ts",
    anchors: { "slots.default": "default", "slots.header": "header" },
    tokens: ["DrawerSlots", "item: string"],
  },
  templateRef: {
    file: "template-ref.ts",
    anchors: { "ref.value": "value" },
    tokens: ["HTMLInputElement", "| null"],
  },
  fallthroughAttrs: {
    file: "fallthrough-attrs.ts",
    anchors: { "attrs.id": "id", "attrs.onClick": "onClick" },
    tokens: ["MouseEvent"],
  },
  autoImportShape: {
    file: "auto-import-shape.ts",
    anchors: { "autoImport.ref": "ref", "autoImport.value": "value" },
    tokens: ["Ref<T>", "value: T"],
  },
  eventArgs: {
    file: "event-args.ts",
    anchors: { "click.event": "event", "keydown.event": "event" },
    tokens: ["MouseEvent", "KeyboardEvent"],
  },
};

function readOracle(file: string): string {
  return readFileSync(
    fileURLToPath(new URL(`../oracles/semantic/${file}`, import.meta.url)),
    "utf-8",
  );
}

describe("curated semantic-oracle `.ts` fixtures", () => {
  it("there is exactly one oracle per required family", () => {
    expect(Object.keys(ORACLES).sort()).toEqual([...ORACLE_FAMILIES].sort());
  });

  for (const family of ORACLE_FAMILIES) {
    const spec = ORACLES[family];
    describe(`${family} (${spec.file})`, () => {
      const source = readOracle(spec.file);

      it("is hermetic — no third-party imports beyond plain TS / DOM lib", () => {
        // Self-contained: the oracle models the intended semantics with plain TS, so
        // it pulls in no `vue`/`@vue` runtime declaration graph.
        expect(/^\s*import\s/m.test(source)).toBe(false);
        expect(source).not.toContain("@dx-anchor }"); // sanity: well-formed markers
      });

      it("contains the intended-semantics tokens", () => {
        for (const token of spec.tokens) expect(source).toContain(token);
      });

      it("resolves every anchor to the START of its target identifier", () => {
        const prepared = prepareOracleSource(source);
        expect(prepared.stripped).not.toContain("@dx-anchor");
        // The offsets are UTF-8 BYTE offsets (the provider/bridge coordinate), so slice
        // the encoded bytes — never the UTF-16 string — to read the targeted token.
        const bytes = Buffer.from(prepared.stripped, "utf-8");
        for (const [anchor, identifier] of Object.entries(spec.anchors)) {
          const offset = prepared.byteOffsets.get(anchor);
          expect(offset, `anchor "${anchor}" present`).toBeDefined();
          const at = bytes
            .subarray(offset!, offset! + Buffer.byteLength(identifier))
            .toString("utf-8");
          expect(at, `anchor "${anchor}" lands on "${identifier}"`).toBe(identifier);
        }
      });
    });
  }
});
