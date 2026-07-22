import { describe, expect, it } from "vitest";

import {
  findAliasProbes,
  findInferredLocalProbes,
  findMemberProbes,
  isTypeScriptCarrier,
  redactIdentifier,
  redactPath,
  scriptRegions,
  selectProbes,
} from "./probes";

/** A synthetic carrier. No corpus content appears anywhere in this lane. */
const SFC = `<template>
  <p>{{ total }}</p>
  <p>{{ invoice.total }}</p>
</template>

<script setup lang="ts">
import { computed, ref } from "vue";
import { formatMoney, type Money } from "./money";
import type { Invoice } from "./invoice";

const invoice = loadInvoice();
const annotated: Money = invoice.total;
const total = computed(() => formatMoney(invoice.total));
const count = ref(0);
</script>
`;

describe("scriptRegions", () => {
  it("returns the script block of a carrier", () => {
    const regions = scriptRegions(SFC);
    expect(regions).toHaveLength(1);
    const body = SFC.slice(regions[0].start, regions[0].end);
    expect(body).toContain("const count = ref(0);");
    expect(body).not.toContain("<template>");
  });

  it("returns the whole file when there is no script tag", () => {
    const plain = "export const x = 1;\n";
    expect(scriptRegions(plain)).toEqual([{ start: 0, end: plain.length }]);
  });

  it("detects a TypeScript carrier", () => {
    expect(isTypeScriptCarrier(SFC)).toBe(true);
    expect(isTypeScriptCarrier("<script setup>\n</script>")).toBe(false);
  });
});

describe("findInferredLocalProbes — the rail-unlocking selector", () => {
  const probes = findInferredLocalProbes(SFC);

  it("emits only annotation-free declarations", () => {
    expect(probes.map((p) => p.identifier)).toEqual(["invoice", "total", "count"]);
    // `annotated: Money` carries an AUTHORED annotation. Verter's native hover
    // formatter re-prints exactly that annotation, producing a payload shaped
    // like a real quickinfo hover, so admitting it would hand the discriminator
    // a false green on its type-position rail.
    expect(probes.map((p) => p.identifier)).not.toContain("annotated");
  });

  it("marks every emitted probe as proven annotation-free", () => {
    expect(probes.every((p) => p.declarationHasNoAuthoredAnnotation === true)).toBe(true);
  });

  it("points at the identifier, not at the declaration keyword", () => {
    for (const probe of probes) {
      expect(SFC.slice(probe.offset, probe.offset + probe.identifier.length)).toBe(
        probe.identifier,
      );
    }
  });

  it("refuses an annotated declaration whatever the annotation looks like", () => {
    const cases = [
      "const a: string = f();",
      "const b: Array<{ x: number }> = f();",
      "let c: Money = f();",
      "const d : Money = f();",
    ];
    for (const source of cases) {
      expect(findInferredLocalProbes(source), source).toEqual([]);
    }
  });

  it("accepts an unannotated declaration with a call initializer", () => {
    const probes = findInferredLocalProbes("const a = f();");
    expect(probes).toHaveLength(1);
    expect(probes[0].declarationHasNoAuthoredAnnotation).toBe(true);
  });
});

describe("findAliasProbes", () => {
  it("finds value specifiers and skips type-only ones", () => {
    const probes = findAliasProbes(SFC);
    expect(probes.map((p) => p.identifier)).toEqual(["computed", "ref", "formatMoney"]);
    expect(probes.map((p) => p.identifier)).not.toContain("Money");
    expect(probes.map((p) => p.identifier)).not.toContain("Invoice");
  });

  it("points at the specifier", () => {
    for (const probe of findAliasProbes(SFC)) {
      expect(SFC.slice(probe.offset, probe.offset + probe.identifier.length)).toBe(
        probe.identifier,
      );
    }
  });

  it("ignores imports outside a script region of a carrier", () => {
    const carrier =
      '<template>\nimport { a } from "b"\n</template>\n<script setup lang="ts">\n</script>';
    expect(findAliasProbes(carrier)).toEqual([]);
  });
});

describe("findMemberProbes", () => {
  it("finds property accesses inside the script region only", () => {
    const probes = findMemberProbes(SFC);
    expect(probes.length).toBeGreaterThan(0);
    expect(probes.every((p) => p.probeClass === "member")).toBe(true);
    for (const probe of probes) {
      expect(SFC.slice(probe.offset, probe.offset + probe.identifier.length)).toBe(
        probe.identifier,
      );
    }
    // The template's `invoice.total` is outside the script region.
    const templateEnd = SFC.indexOf("</template>");
    expect(probes.every((p) => p.offset > templateEnd)).toBe(true);
  });

  it("skips numeric receivers but keeps a spread member", () => {
    expect(findMemberProbes("const a = 1.5;")).toEqual([]);
    expect(findMemberProbes("const a = { ...b.c };").map((p) => p.identifier)).toEqual(["c"]);
  });

  it("never probes inside a string literal or a line comment", () => {
    // `"./money.ts"` reads as `money.ts` to a naive member regex. A probe there
    // can only ever return empty, which would be recorded as a defect in a lane
    // that treats every empty as a defect.
    expect(findMemberProbes('import { x } from "./money.ts";')).toEqual([]);
    expect(findMemberProbes("// see invoice.total for details")).toEqual([]);
    expect(findInferredLocalProbes("// const a = f();")).toEqual([]);
    expect(findInferredLocalProbes('const s = "const b = g();";').map((p) => p.identifier)).toEqual(
      ["s"],
    );
  });
});

describe("selectProbes", () => {
  it("bounds each class and stays deterministic", () => {
    const first = selectProbes(SFC, 2);
    const second = selectProbes(SFC, 2);
    expect(first).toEqual(second);
    for (const cls of ["member", "alias", "inferred-local"] as const) {
      expect(first.filter((p) => p.probeClass === cls).length).toBeLessThanOrEqual(2);
    }
  });
});

describe("member probe positions are usable cursor targets", () => {
  it("returns offsets that land on the identifier itself", () => {
    for (const probe of selectProbes(SFC, 3)) {
      expect(SFC.slice(probe.offset, probe.offset + probe.identifier.length)).toBe(
        probe.identifier,
      );
    }
  });
});

describe("redaction", () => {
  it("never reproduces the input path", () => {
    const redacted = redactPath("src/components/Invoice.vue");
    expect(redacted).not.toContain("Invoice");
    expect(redacted).not.toContain("components");
    expect(redacted).not.toContain("src");
    expect(redacted.endsWith(".vue")).toBe(true);
    expect(redacted).toMatch(/^[0-9a-f]{12}\.vue$/);
  });

  it("is stable and collision-distinct", () => {
    expect(redactPath("a/b.ts")).toBe(redactPath("a/b.ts"));
    expect(redactPath("a/b.ts")).not.toBe(redactPath("a/c.ts"));
  });

  it("redacts identifiers", () => {
    expect(redactIdentifier("invoiceTotal")).toMatch(/^[0-9a-f]{8}$/);
    expect(redactIdentifier("invoiceTotal")).not.toContain("invoice");
  });
});
