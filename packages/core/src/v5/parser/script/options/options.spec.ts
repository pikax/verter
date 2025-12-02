import { parseAST } from "../../ast";
import { parser } from "../../parser";
import { deepWalk, shallowWalk } from "../../walk";
import { ScriptItem, ScriptTypes } from "../types";
import { createOptionsContext, handleOptionsNode } from "./index.js";
import { createSetupContext } from "../setup/index.js";

describe("parser script options", () => {
  function parse(source: string) {
    const items: ScriptItem[] = [];
    const ast = parseAST(source);

    let isAsync = false;
    shallowWalk(ast, (node) => {
      //   const shared = handleShared(node);
      //   if (shared) {
      //     items.push(...shared);
      //   }
      const result = handleOptionsNode(node);
      if (Array.isArray(result)) {
        items.push(...result);
      } else {
        isAsync = result.isAsync;
        items.push(...result.items);
      }
    });

    return {
      items,
      isAsync,
    };
  }

  function parseWithContext(source: string, lang: "ts" | "js" = "ts") {
    const ast = parseAST(source);
    const setupCtx = createSetupContext({ lang });
    const optionsCtx = createOptionsContext({ lang, setupCtx });

    const items: ScriptItem[] = [];

    function addResult(result: void | ScriptItem | ScriptItem[]) {
      if (!result) return;
      if (Array.isArray(result)) {
        items.push(...result);
      } else {
        items.push(result);
      }
    }

    deepWalk(
      ast,
      function (node, parent, key) {
        const visit = optionsCtx.visit(node, parent, key);
        addResult(visit);
      },
      (node, parent, key) => {
        optionsCtx.leave(node, parent, key);
      }
    );

    return {
      items,
      isAsync: setupCtx.isAsync,
    };
  }

  describe("async", () => {
    it("not async", () => {
      const { isAsync } = parse(`export default {}`);
      expect(isAsync).toBe(false);
    });

    it("async {}", () => {
      const { isAsync } = parse(`export default async {}`);
      expect(isAsync).toBe(false);
    });

    it("{ setup() { await Promise.resolve} }", () => {
      const { isAsync } = parse(
        `export default { setup() { await Promise.resolve} }`
      );
      expect(isAsync).toBe(false);
    });

    it("{ async setup() { }", () => {
      const { isAsync } = parse(`export default { async setup() { }}`);
      expect(isAsync).toBe(true);
    });

    describe.each(["defineComponent", "myWrapperFunction"])(
      "wrapper %s",
      (wrapper) => {
        it("not async", () => {
          const { isAsync } = parse(`export default ${wrapper}({})`);
          expect(isAsync).toBe(false);
        });

        it("async {}", () => {
          const { isAsync } = parse(`export default ${wrapper}(async {})`);
          expect(isAsync).toBe(false);
        });

        it("{ setup() { await Promise.resolve} }", () => {
          const { isAsync } = parse(
            `export default ${wrapper}({ setup() { await Promise.resolve} })`
          );
          expect(isAsync).toBe(false);
        });

        it("{ async setup() { }", () => {
          const { isAsync } = parse(
            `export default ${wrapper}({ async setup() { }})`
          );
          expect(isAsync).toBe(true);
        });
      }
    );
  });

  /**
   * @ai-generated - Tests for createOptionsContext function.
   * Verifies that the context properly visits and collects items inside
   * defineComponent setup function, including arrow function syntax.
   */
  describe("createOptionsContext", () => {
    describe("setup function detection", () => {
      it("visits function calls inside setup() function expression", () => {
        const { items } = parseWithContext(
          `export default defineComponent({ setup() { const a = useTemplateRef(); return {} } })`
        );

        const functionCalls = items.filter(
          (x) => x.type === ScriptTypes.FunctionCall
        );
        expect(functionCalls).toHaveLength(1);
        expect(functionCalls[0]).toMatchObject({
          type: ScriptTypes.FunctionCall,
          name: "useTemplateRef",
        });
      });

      it("visits function calls inside setup: () => {} arrow function", () => {
        const { items } = parseWithContext(
          `export default defineComponent({ setup: () => { const a = ref(0); return {} } })`
        );

        const functionCalls = items.filter(
          (x) => x.type === ScriptTypes.FunctionCall
        );
        expect(functionCalls.some((x) => x.name === "ref")).toBe(true);
      });

      it("visits multiple function calls inside setup", () => {
        const { items } = parseWithContext(
          `export default defineComponent({ setup() { ref(); useTemplateRef(); computed(); return {} } })`
        );

        const functionCalls = items.filter(
          (x) => x.type === ScriptTypes.FunctionCall
        );
        expect(functionCalls.map((x) => x.name)).toContain("ref");
        expect(functionCalls.map((x) => x.name)).toContain("useTemplateRef");
        expect(functionCalls.map((x) => x.name)).toContain("computed");
      });

      it("does not visit function calls outside setup", () => {
        const { items } = parseWithContext(
          `outsideCall(); export default defineComponent({ setup() { insideCall(); return {} } })`
        );

        const functionCalls = items.filter(
          (x) => x.type === ScriptTypes.FunctionCall
        );
        // Only insideCall should be captured
        expect(functionCalls).toHaveLength(1);
        expect(functionCalls[0].name).toBe("insideCall");
      });

      it("handles plain object export without wrapper", () => {
        const { items } = parseWithContext(
          `export default { setup() { myFunc(); return {} } }`
        );

        const functionCalls = items.filter(
          (x) => x.type === ScriptTypes.FunctionCall
        );
        expect(functionCalls).toHaveLength(1);
        expect(functionCalls[0].name).toBe("myFunc");
      });

      it("handles arrow function setup with block body", () => {
        const { items } = parseWithContext(
          `export default defineComponent({ setup: () => { useTemplateRef(); return {} } })`
        );

        const functionCalls = items.filter(
          (x) => x.type === ScriptTypes.FunctionCall
        );
        expect(functionCalls.some((x) => x.name === "useTemplateRef")).toBe(
          true
        );
      });

      it("handles custom wrapper functions", () => {
        const { items } = parseWithContext(
          `export default myCustomWrapper({ setup() { useTemplateRef(); return {} } })`
        );

        const functionCalls = items.filter(
          (x) => x.type === ScriptTypes.FunctionCall
        );
        expect(functionCalls.some((x) => x.name === "useTemplateRef")).toBe(
          true
        );
      });

      it("captures default export", () => {
        const { items } = parseWithContext(
          `export default defineComponent({ setup() { return {} } })`
        );

        const defaultExports = items.filter(
          (x) => x.type === ScriptTypes.DefaultExport
        );
        expect(defaultExports).toHaveLength(1);
      });
    });
  });
});
