import { describe, expect, it } from "vitest";

import {
  applyResolvedCompletion,
  applyTextEdits,
  completionItemEdits,
  parseImportDeclarations,
  verifyAutoImport,
} from "../src/collectors/index.js";
import type { CollectorEventKey } from "../src/collectors/index.js";
import type { CanonicalCompletionItem, TextEdit } from "../src/index.js";

const key: CollectorEventKey = {
  scenario: "auto-import",
  editStepIndex: 0,
  driver: "rawLsp",
  provider: "tsgo",
  probe: "auto-import-foo",
  version: 2,
  anchor: "cursor",
};

const before = "import { ref } from 'vue'\nconst a = 1\n";

const importEdit = (newText: string, line = 1): TextEdit => ({
  range: { start: { line, character: 0 }, end: { line, character: 0 } },
  newText,
});

describe("applyTextEdits — non-overlapping edits applied right-to-left", () => {
  it("inserts two edits at distinct positions without invalidating earlier offsets", () => {
    const edits: TextEdit[] = [
      { range: { start: { line: 0, character: 0 }, end: { line: 0, character: 0 } }, newText: "A" },
      { range: { start: { line: 1, character: 0 }, end: { line: 1, character: 0 } }, newText: "B" },
    ];
    expect(applyTextEdits(before, edits)).toBe("Aimport { ref } from 'vue'\nBconst a = 1\n");
  });

  it("applies a replacement range (delete + insert) at one site", () => {
    // Line 1 "const a = 1": the literal "1" is at column 10.
    const edits: TextEdit[] = [
      {
        range: { start: { line: 1, character: 10 }, end: { line: 1, character: 11 } },
        newText: "42",
      },
    ];
    expect(applyTextEdits(before, edits)).toBe("import { ref } from 'vue'\nconst a = 42\n");
  });

  it("throws on OVERLAPPING edits (the right-to-left splice presumes non-overlap)", () => {
    const edits: TextEdit[] = [
      { range: { start: { line: 0, character: 0 }, end: { line: 0, character: 5 } }, newText: "X" },
      { range: { start: { line: 0, character: 3 }, end: { line: 0, character: 8 } }, newText: "Y" },
    ];
    expect(() => applyTextEdits("0123456789", edits)).toThrow(/overlap/i);
  });

  it("accepts ABUTTING edits (one ends exactly where the next begins — half-open, no overlap)", () => {
    const edits: TextEdit[] = [
      { range: { start: { line: 0, character: 0 }, end: { line: 0, character: 3 } }, newText: "A" },
      { range: { start: { line: 0, character: 3 }, end: { line: 0, character: 6 } }, newText: "B" },
    ];
    expect(applyTextEdits("0123456789", edits)).toBe("AB6789");
  });
});

describe("completionItemEdits — unwraps the main edit + additionalTextEdits", () => {
  it("collects an InsertReplaceEdit (via its replace range) plus additionalTextEdits", () => {
    const item: CanonicalCompletionItem = {
      label: "Foo",
      textEdit: {
        newText: "Foo",
        insert: { start: { line: 1, character: 6 }, end: { line: 1, character: 6 } },
        replace: { start: { line: 1, character: 6 }, end: { line: 1, character: 9 } },
      },
      additionalTextEdits: [importEdit("import Foo from './Foo.vue'\n")],
    };
    const edits = completionItemEdits(item);
    expect(edits).toHaveLength(2);
    expect(edits.some((e) => e.newText.includes("import Foo"))).toBe(true);
    // the InsertReplaceEdit contributes its replace range
    expect(edits.some((e) => e.range.end.character === 9)).toBe(true);
  });
});

describe("parseImportDeclarations — module + bound names per contiguous declaration", () => {
  it("parses a NAMED import to its module and exact bound symbol", () => {
    expect(parseImportDeclarations('import { helperValue } from "./helper"\n')).toEqual([
      { module: "./helper", bindings: ["helperValue"] },
    ]);
  });

  it("parses a DEFAULT import", () => {
    expect(parseImportDeclarations("import Foo from './Foo.vue'\n")).toEqual([
      { module: "./Foo.vue", bindings: ["Foo"] },
    ]);
  });

  it("binds the ALIAS of a renamed named import (`x as y` → `y`)", () => {
    expect(parseImportDeclarations('import { original as helperValue } from "./helper"')).toEqual([
      { module: "./helper", bindings: ["helperValue"] },
    ]);
  });

  it("binds a NAMESPACE import alias (`* as ns`)", () => {
    expect(parseImportDeclarations('import * as ns from "./m"')).toEqual([
      { module: "./m", bindings: ["ns"] },
    ]);
  });

  it("binds the EXACT name, never a prefix of a longer identifier", () => {
    // `helperValueExtra` must NOT be reported as binding `helperValue`.
    const [decl] = parseImportDeclarations('import { helperValueExtra } from "./helper"');
    expect(decl.bindings).toEqual(["helperValueExtra"]);
    expect(decl.bindings.includes("helperValue")).toBe(false);
  });

  it("tolerates a MULTI-LINE named clause as one declaration", () => {
    expect(parseImportDeclarations('import {\n  helperValue,\n} from "./helper"\n')).toEqual([
      { module: "./helper", bindings: ["helperValue"] },
    ]);
  });

  it("SKIPS a side-effect import (no `from`) and never stitches it into the next declaration", () => {
    // The `[^;]` clause stops at the `;`, so the side-effect import's missing `from`
    // cannot borrow the next statement's `from "./m"`.
    expect(parseImportDeclarations('import "./side-effect";\nimport { a } from "./m"')).toEqual([
      { module: "./m", bindings: ["a"] },
    ]);
  });
});

describe("verifyAutoImport — the resolved edit must STRUCTURALLY bind the exact symbol from the exact module", () => {
  it("PASSES when the applied named import binds the exact symbol from the exact module", () => {
    const item: CanonicalCompletionItem = {
      label: "helperValue",
      additionalTextEdits: [importEdit('import { helperValue } from "./helper"\n')],
    };
    const after = applyResolvedCompletion(before, item);
    expect(after).toContain('import { helperValue } from "./helper"');

    const events = verifyAutoImport({
      key,
      before,
      item,
      expectedImport: { symbol: "helperValue", module: "./helper" },
    });
    expect(events.every((e) => e.ok)).toBe(true);
    expect(events.some((e) => e.signal === "auto_import_applied" && e.ok)).toBe(true);
  });

  it("PASSES a DEFAULT import binding the expected symbol from the expected module", () => {
    const item: CanonicalCompletionItem = {
      label: "Foo",
      additionalTextEdits: [importEdit("import Foo from './Foo.vue'\n")],
    };
    const events = verifyAutoImport({
      key,
      before,
      item,
      expectedImport: { symbol: "Foo", module: "./Foo.vue" },
    });
    expect(events.some((e) => e.signal === "auto_import_applied" && e.ok)).toBe(true);
  });

  it("FLAGS an empty edit (resolve produced no import) as user-visible", () => {
    const item: CanonicalCompletionItem = { label: "helperValue" };
    const events = verifyAutoImport({
      key,
      before,
      item,
      expectedImport: { symbol: "helperValue", module: "./helper" },
    });
    const fail = events.filter((e) => !e.ok);
    expect(fail.length).toBeGreaterThan(0);
    expect(fail[0].severity).toBe("userVisible");
    expect(fail.some((e) => e.signal === "auto_import_empty_edit")).toBe(true);
  });

  it("FLAGS a WRONG SYMBOL: a same-module import that binds a different name", () => {
    // `import { other } from "./helper"` — the module is correct but the symbol is not.
    const item: CanonicalCompletionItem = {
      label: "helperValue",
      additionalTextEdits: [importEdit('import { other } from "./helper"\n')],
    };
    const events = verifyAutoImport({
      key,
      before,
      item,
      expectedImport: { symbol: "helperValue", module: "./helper" },
    });
    const fail = events.filter((e) => !e.ok);
    expect(fail.some((e) => e.signal === "auto_import_wrong_text")).toBe(true);
    expect(fail.every((e) => e.severity === "userVisible")).toBe(true);
    expect(events.some((e) => e.signal === "auto_import_applied")).toBe(false);
  });

  it("FLAGS a WRONG MODULE whose specifier merely CONTAINS the expected one", () => {
    // `import { helperValue } from "./helper-extra"` — `./helper-extra` contains the
    // substring `./helper`, so an `after.includes("./helper")` scan would WRONGLY pass;
    // the exact-module structural check fails it.
    const item: CanonicalCompletionItem = {
      label: "helperValue",
      additionalTextEdits: [importEdit('import { helperValue } from "./helper-extra"\n')],
    };
    const after = applyResolvedCompletion(before, item);
    expect(after).toContain("./helper"); // the substring trap a whole-buffer scan falls into

    const events = verifyAutoImport({
      key,
      before,
      item,
      expectedImport: { symbol: "helperValue", module: "./helper" },
    });
    const fail = events.filter((e) => !e.ok);
    expect(fail.some((e) => e.signal === "auto_import_wrong_text")).toBe(true);
    expect(events.some((e) => e.signal === "auto_import_applied")).toBe(false);
  });

  it("FLAGS a binding the resolved item did NOT introduce — a pre-existing same import", () => {
    // `before` ALREADY imports `helperValue` from `./helper`. The resolved item applies
    // ONLY a usage insertion (a main `textEdit`, no import-adding edit), so the buffer
    // changes (the empty-edit guard does NOT fire) yet the item bound nothing new. A
    // whole-buffer scan finds the pre-existing import and WRONGLY reports `applied`;
    // gating on the before→after binding delta flags it instead.
    const beforeWithImport = 'import { helperValue } from "./helper"\nconst a = 1\n';
    const item: CanonicalCompletionItem = {
      label: "helperValue",
      textEdit: {
        range: { start: { line: 1, character: 10 }, end: { line: 1, character: 10 } },
        newText: "helperValue",
      },
    };
    // The edit is non-empty and the buffer genuinely changes, so NEITHER empty-edit
    // condition fires — but the item introduces no import declaration.
    expect(completionItemEdits(item)).toHaveLength(1);
    const after = applyResolvedCompletion(beforeWithImport, item);
    expect(after).not.toBe(beforeWithImport);
    expect(after).toContain('import { helperValue } from "./helper"');

    const events = verifyAutoImport({
      key,
      before: beforeWithImport,
      item,
      expectedImport: { symbol: "helperValue", module: "./helper" },
    });
    const fail = events.filter((e) => !e.ok);
    expect(fail.length).toBeGreaterThan(0);
    expect(fail.every((e) => e.severity === "userVisible")).toBe(true);
    expect(fail.some((e) => e.signal === "auto_import_not_introduced")).toBe(true);
    expect(events.some((e) => e.signal === "auto_import_applied")).toBe(false);
  });

  // The EXACT resolved-edit text the real providers emit for the
  // `providerResolveParity` integration scenario (an unimported `myHelper` from
  // `./helper`). Captured from a live tsgo/tsserver run so the pure collector is
  // pinned to the real provider output shape, including tsserver's CRLF.
  const realImportEdit = (newText: string): CanonicalCompletionItem => ({
    label: "myHelper",
    additionalTextEdits: [importEdit(newText)],
  });
  const realScenarioBefore = "myHelper\n";
  const realExpected = { symbol: "myHelper", module: "./helper" } as const;

  it("ACCEPTS the real tsgo resolved import edit (LF) as a bound auto-import", () => {
    // tsgo emitted: `import { myHelper } from "./helper";\n\n`
    const item = realImportEdit('import { myHelper } from "./helper";\n\n');
    const events = verifyAutoImport({
      key,
      before: realScenarioBefore,
      item,
      expectedImport: realExpected,
    });
    expect(events.every((e) => e.ok)).toBe(true);
    expect(events.some((e) => e.signal === "auto_import_applied" && e.ok)).toBe(true);
  });

  it("ACCEPTS the real tsserver resolved import edit (CRLF) — provider parity at the collector", () => {
    // tsserver emitted the SAME import with CRLF: `import { myHelper } from "./helper";\r\n\r\n`
    const item = realImportEdit('import { myHelper } from "./helper";\r\n\r\n');
    const events = verifyAutoImport({
      key,
      before: realScenarioBefore,
      item,
      expectedImport: realExpected,
    });
    expect(events.every((e) => e.ok)).toBe(true);
    expect(events.some((e) => e.signal === "auto_import_applied" && e.ok)).toBe(true);
  });

  it("PASSES a merge that NEWLY binds the symbol in an existing same-module import", () => {
    // `before` imports a DIFFERENT name from `./helper`; the resolved item rewrites that
    // import to ALSO bind `helperValue`. The binding is new (absent in `before`, present
    // in `after`), so it is a real applied auto-import even though no new import LINE is
    // added — the before→after delta must not over-reject this genuine merge.
    const beforeWithOther = 'import { other } from "./helper"\nconst a = 1\n';
    const item: CanonicalCompletionItem = {
      label: "helperValue",
      additionalTextEdits: [
        {
          // Insert `, helperValue` right after `other`, before the closing ` }`.
          range: { start: { line: 0, character: 14 }, end: { line: 0, character: 14 } },
          newText: ", helperValue",
        },
      ],
    };
    const after = applyResolvedCompletion(beforeWithOther, item);
    expect(after).toContain('import { other, helperValue } from "./helper"');

    const events = verifyAutoImport({
      key,
      before: beforeWithOther,
      item,
      expectedImport: { symbol: "helperValue", module: "./helper" },
    });
    expect(events.every((e) => e.ok)).toBe(true);
    expect(events.some((e) => e.signal === "auto_import_applied" && e.ok)).toBe(true);
  });
});
