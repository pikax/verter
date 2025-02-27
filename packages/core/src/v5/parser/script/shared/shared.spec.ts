import { parseAST } from "../../ast";
import { parser } from "../../parser";
import { deepWalk } from "../../walk";
import { ScriptItem, ScriptTypes } from "../types";
import { createSharedContext } from "./index.js";

describe("parser script setup", () => {
  function parse(source: string, lang = "js") {
    const context = createSharedContext({ lang });
    const items = [] as ScriptItem[];

    const ast = parseAST(source);

    deepWalk(ast, (node, parent) => {
      const r = context.visit(node, parent);
      if (r) {
        if (Array.isArray(r)) {
          items.push(...r);
        } else {
          items.push(r);
        }
      }
    });

    return {
      items,
    };
  }

  describe("export", () => {
    test("export const a = 1", () => {
      const { items } = parse(`export const a = 1`);
      expect(items).toMatchObject([
        {
          type: ScriptTypes.Export,
        },
      ]);
    });
  });

  describe("import", () => {
    test('import { a } from "b"', () => {
      const { items } = parse(`import { a } from "b"`);
      expect(items).toMatchObject([
        {
          type: ScriptTypes.Import,
          bindings: [
            {
              type: ScriptTypes.Binding,
              name: "a",
            },
          ],
        },
      ]);
    });
    test('import * as a from "b"', () => {
      const { items } = parse(`import * as a from "b"`);
      expect(items).toMatchObject([
        {
          type: ScriptTypes.Import,
          bindings: [
            {
              type: ScriptTypes.Binding,
              name: "a",
            },
          ],
        },
      ]);
    });
  });

  describe("type assertion", () => {
    test("const a = <number>1", () => {
      const { items } = parse(`const a = <number>1`);
      expect(items).toMatchObject([
        {
          type: ScriptTypes.TypeAssertion,
        },
      ]);
    });

    test("const a = 1 as number", () => {
      const { items } = parse(`const a = 1 as number`);
      expect(items).toMatchObject([]);
    });

    test("const a = { foo: <{a: 1}>{}}", () => {
      const { items } = parse(`const a = { foo: <{a: 1}>{}}`);
      expect(items).toMatchObject([
        {
          type: ScriptTypes.TypeAssertion,
        },
      ]);
    });
  });
});
