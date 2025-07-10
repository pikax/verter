import { parseAST } from "../../ast";
import { parser } from "../../parser";
import { shallowWalk } from "../../walk";
import { ScriptItem, ScriptTypes } from "../types";
import { handleOptionsNode } from "./index.js";

describe("parser script setup", () => {
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
});
