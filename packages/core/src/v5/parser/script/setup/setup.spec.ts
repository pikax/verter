import { parseAST } from "../../ast";
import { parser } from "../../parser";
import { shallowWalk } from "../../walk";
import { ScriptItem, ScriptTypes } from "../types";
import { handleSetupNode } from "./index.js";

describe("parser script setup", () => {
  function parse(source: string) {
    const items: ScriptItem[] = [];
    let isAsync = false;
    const ast = parseAST(source);

    shallowWalk(ast, (node) => {
      //   const shared = handleShared(node);
      //   if (shared) {
      //     items.push(...shared);
      //   }
      const result = handleSetupNode(node);
      if (result.isAsync) {
        isAsync = result.isAsync;
      }
      if (Array.isArray(result)) {
        items.push(...result);
      } else {
        items.push(...result.items);
      }
    });

    return {
      isAsync,
      items,
    };
  }

  it("let a = 0", () => {
    const { items } = parse(`let a = 0`);

    expect(items).toMatchObject([
      {
        type: ScriptTypes.Declaration,
        name: "a",
        rest: false,
      },
    ]);
  });

  it("const props = defineProps()", () => {
    const { items } = parse(`const props = defineProps()`);
    expect(items).toMatchObject([
      {
        type: ScriptTypes.Declaration,
        name: "props",
        rest: false,
        parent: {
          init: {
            callee: {
              name: "defineProps",
            },
          },
        },
      },
    ]);
  });

  it('const props = withDefaults(defineProps(), { a: "a" })', () => {
    const { items } = parse(`const props = withDefaults(defineProps(), { a: "a" })`);
    expect(items).toMatchObject([
      {
        type: ScriptTypes.Declaration,
        name: "props",
        rest: false,
        parent: {
          init: {
            callee: {
              name: "withDefaults",
            },
          },
        },
      },
    ]);
  });

  it('withDefaults(defineProps(), { a: "a" })', () => {
    const { items } = parse(`withDefaults(defineProps(), { a: "a" })`);
    expect(items).toMatchObject([
      {
        type: ScriptTypes.FunctionCall,
        name: "withDefaults",
      },
    ]);
  });

  it("const { a } = defineProps()", () => {
    const { items } = parse(`const { a } = defineProps()`);
    expect(items).toMatchObject([
      {
        type: ScriptTypes.Declaration,
        name: "a",
        parent: {
          init: {
            callee: {
              name: "defineProps",
            },
          },
        },
      },
    ]);
  });

  it("const { a } : Props = defineProps()", () => {
    const { items } = parse(`const { a } : Props = defineProps()`);
    expect(items).toMatchObject([
      {
        type: ScriptTypes.Declaration,
        name: "a",
        parent: {
          init: {
            callee: {
              name: "defineProps",
            },
          },
        },
      },
    ]);
  });

  it("const { foo: { bar: a } } : Props = defineProps()", () => {
    const { items } = parse(`const { foo: { bar: a } } : Props = defineProps()`);
    expect(items).toMatchObject([
      {
        type: ScriptTypes.Declaration,
        name: "a",
        parent: {
          init: {
            callee: {
              name: "defineProps",
            },
          },
        },
      },
    ]);
  });

  it("const { foo: { bar: { a = 1 } } } : Props = defineProps()", () => {
    const { items } = parse(`const { foo: { bar: { a = 1 } } } : Props = defineProps()`);
    expect(items).toMatchObject([
      {
        type: ScriptTypes.Declaration,
        name: "a",
        parent: {
          init: {
            callee: {
              name: "defineProps",
            },
          },
        },
      },
    ]);
  });

  it("const { a } = defineProps({ a: String, foo: String})", () => {
    const { items } = parse(`const { a } = defineProps({ a: String, foo: String})`);
    expect(items).toMatchObject([
      {
        type: ScriptTypes.Declaration,
        name: "a",
        parent: {
          init: {
            callee: {
              name: "defineProps",
            },
          },
        },
      },
    ]);
  });

  it("{ const a = 0 } }", () => {
    const { items } = parse("{ const a = 0 } }");
    expect(items).toMatchObject([]);
  });

  it("function foo() { const a = 0 }", () => {
    const { items } = parse("function foo() { const a = 0 }");
    expect(items).toMatchObject([
      {
        type: ScriptTypes.Declaration,
        name: "foo",
        async: false,
        expression: false,
        generator: false,
        returnType: null,
        params: [],
        body: {
          type: "BlockStatement",
        },
        typeParameters: null,
      },
    ]);
  });

  describe("declare", () => {
    it("declare let a: number", () => {
      const { items } = parse(`declare let a: number`);
      expect(items).toMatchObject([
        {
          type: ScriptTypes.Declaration,
          name: "a",
          rest: false,
          declare: true,
        },
      ]);
    });

    it("declare const b: string", () => {
      const { items } = parse(`declare const b: string`);
      expect(items).toMatchObject([
        {
          type: ScriptTypes.Declaration,
          name: "b",
          rest: false,
          declare: true,
        },
      ]);
    });

    it("declare function foo(): void", () => {
      const { items } = parse(`declare function foo(): void`);
      expect(items).toMatchObject([
        {
          type: ScriptTypes.Declaration,
          name: "foo",
          declare: true,
          async: false,
          expression: false,
          generator: false,
          params: [],
          body: null,
          returnType: {
            type: "TSTypeAnnotation",
            typeAnnotation: {
              type: "TSVoidKeyword",
            },
          },
        },
      ]);
    });
  });

  describe("async", () => {
    it("await Promise.resolve()", () => {
      const { items } = parse(`await Promise.resolve()`);
      expect(items).toMatchObject([
        {
          type: ScriptTypes.Async,
        },
        {
          type: ScriptTypes.FunctionCall,
          name: "resolve",
        },
      ]);
    });

    it("const a = await Promise.resolve()", () => {
      const { items } = parse(`const a = await Promise.resolve()`);
      expect(items).toMatchObject([
        {
          type: ScriptTypes.Async,
        },
        {
          type: ScriptTypes.Declaration,
          name: "a",
        },
      ]);
    });

    it("if(true) { await Promise.resolve() }", () => {
      const { items } = parse(`if(true) { await Promise.resolve() }`);
      expect(items).toMatchObject([
        {
          type: ScriptTypes.Async,
        },
        {
          type: ScriptTypes.FunctionCall,
          name: "resolve",
        },
      ]);
    });

    it("if(true){} else { await Promise.resolve() }", () => {
      const { items } = parse(`if(true){} else { await Promise.resolve() }`);
      expect(items).toMatchObject([
        {
          type: ScriptTypes.Async,
        },
        {
          type: ScriptTypes.FunctionCall,
          name: "resolve",
        },
      ]);
    });

    it("if(true) { { await Promise.resolve() } }", () => {
      const { items } = parse(`if(true) { { await Promise.resolve() } }`);
      expect(items).toMatchObject([
        {
          type: ScriptTypes.Async,
        },
        {
          type: ScriptTypes.FunctionCall,
          name: "resolve",
        },
      ]);
    });

    it("for(let i; i < 10; i++) { await Promise.resolve() }", () => {
      const { items } = parse("for(let i; i < 10; i++) { await Promise.resolve() }");
      expect(items).toMatchObject([
        {
          type: ScriptTypes.Async,
        },
        {
          type: ScriptTypes.FunctionCall,
          name: "resolve",
        },
      ]);
    });

    it("async function", () => {
      const { isAsync } = parse(`<script setup lang="ts">
async function bindGesture(el?: HTMLElement | null) {
  if (!el) {
    return;
  }
  await waitUntilNotBusy();
}
</script>
<template></template>
`);

      expect(isAsync).toBe(false);
    });
  });
});
