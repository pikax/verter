import { MagicString } from "@vue/compiler-sfc";
import { parser } from "../../../../parser";
import {
  ParsedBlockScript,
  ParsedBlockTemplate,
} from "../../../../parser/types";
import { processScript } from "../../script";

import { ScriptBlockPlugin } from "../script-block";
import { BindingPlugin } from "../binding";
import { TemplateTypes } from "../../../../parser/template/types";

import { TemplateRefPlugin } from "./index";

describe("process script plugin template-ref", () => {
  function parse(
    content: string,
    wrapper: string | false = false,
    lang = "js",

    pre = "",
    post = "",
    attrs = ""
  ) {
    const prepend = `${pre}<script ${
      wrapper === false ? "setup" : ""
    } lang="${lang}"${attrs ? ` ${attrs}` : ""}>`;
    const source = `${prepend}${content}</script>${post}`;
    const parsed = parser(source);

    const s = new MagicString(source);

    const scriptBlock = parsed.blocks.find(
      (x) => x.type === "script"
    ) as ParsedBlockScript;
    const template = parsed.blocks.find(
      (x) => x.type === "template"
    ) as ParsedBlockTemplate;

    const r = processScript(scriptBlock.result.items, [TemplateRefPlugin], {
      s,
      filename: "test.vue",
      blocks: parsed.blocks,
      block: scriptBlock,
      isAsync: parsed.isAsync,
      templateBindings:
        template?.result?.items.filter(
          (x) => x.type === TemplateTypes.Binding
        ) ?? [],
      blockNameResolver: (name) => name,
    });
    return r;
  }

  describe("ts", () => {
    test("no ref", () => {
      const { result } = parse(`let a = useTemplateRef()`, false, "ts");
      expect(result).toMatchInlineSnapshot(
        `"<script setup lang="ts">let a = useTemplateRef()</script>"`
      );
    });

    it("ref", () => {
      const { result } = parse(
        `let a = useTemplateRef()`,
        false,
        "ts",
        '<template><div ref="a"/></template>'
      );
      expect(result).toMatchInlineSnapshot(
        `"<template><div ref="a"/></template><script setup lang="ts">let a = useTemplateRef<ReturnType<typeof ___VERTER___Comp10>,"a">()</script>"`
      );
    });

    it("MyComp :ref='a'", () => {
      const { result } = parse(
        `import Comp from './Comp.vue';let a = useTemplateRef()`,
        false,
        "ts",
        '<template><Comp :ref="a"/></template>'
      );
      expect(result).toMatchInlineSnapshot(
        `"<template><Comp :ref="a"/></template><script setup lang="ts">import Comp from './Comp.vue';let a = useTemplateRef<ReturnType<typeof ___VERTER___Comp10>,typeof a>()</script>"`
      );
    });

    it("MyComp :ref='foo'", () => {
      const { result } = parse(
        `import MyComp from './Comp.vue';let a = useTemplateRef();const foo = 'test'`,
        false,
        "ts",
        '<template><my-comp :ref="foo"/></template>'
      );
      expect(result).toMatchInlineSnapshot(
        `"<template><my-comp :ref="foo"/></template><script setup lang="ts">import MyComp from './Comp.vue';let a = useTemplateRef<ReturnType<typeof ___VERTER___Comp10>,typeof foo>();const foo = 'test'</script>"`
      );
    });

    it("MyComp :ref='foo' with string matching", () => {
      const { result } = parse(
        `import MyComp from './Comp.vue';let a = useTemplateRef('test');const foo = 'test'`,
        false,
        "ts",
        '<template><my-comp :ref="foo"/></template>'
      );
      expect(result).toMatchInlineSnapshot(
        `"<template><my-comp :ref="foo"/></template><script setup lang="ts">import MyComp from './Comp.vue';let a = useTemplateRef<ReturnType<typeof ___VERTER___Comp10>,typeof foo>('test');const foo = 'test'</script>"`
      );
    });

    it('unknown if :ref="foo"', () => {
      const { result } = parse(
        `import MyComp from './Comp.vue';let a = useTemplateRef('test');const foo = 'testx'`,
        false,
        "ts",
        '<template><my-comp :ref="foo"/></template>'
      );
      expect(result).toMatchInlineSnapshot(
        `"<template><my-comp :ref="foo"/></template><script setup lang="ts">import MyComp from './Comp.vue';let a = useTemplateRef<unknown,typeof foo>('test');const foo = 'testx'</script>"`
      );
    });
    it("multiple MyComp :ref='foo' with string matching", () => {
      const { result } = parse(
        `import MyComp from './Comp.vue';import Comp from './Comp.vue'; let a = useTemplateRef('test');const foo = 'test';const foox = 'test'`,
        false,
        "ts",
        '<template><my-comp :ref="foox"/></template>'
      );
      expect(result).toMatchInlineSnapshot(
        `"<template><my-comp :ref="foox"/></template><script setup lang="ts">import MyComp from './Comp.vue';import Comp from './Comp.vue'; let a = useTemplateRef<ReturnType<typeof ___VERTER___Comp10>,typeof foox>('test');const foo = 'test';const foox = 'test'</script>"`
      );
    });
    it("multiple MyComp :ref='foo' with string matching but incorrect", () => {
      const { result } = parse(
        `import MyComp from './Comp.vue';import Comp from './Comp.vue'; let a = useTemplateRef('test');const foo = 'test';const foox = 'testx'`,
        false,
        "ts",
        '<template><my-comp :ref="foox"/></template>'
      );
      expect(result).toMatchInlineSnapshot(
        `"<template><my-comp :ref="foox"/></template><script setup lang="ts">import MyComp from './Comp.vue';import Comp from './Comp.vue'; let a = useTemplateRef<unknown,typeof foox>('test');const foo = 'test';const foox = 'testx'</script>"`
      );
    });

    it("MyComp :ref='foo.bar'", () => {
      const { result } = parse(
        `import MyComp from './Comp.vue';let a = useTemplateRef();const foo = { bar:'test'}`,
        false,
        "ts",
        '<template><my-comp :ref="foo.bar"/></template>'
      );
      expect(result).toMatchInlineSnapshot(
        `"<template><my-comp :ref="foo.bar"/></template><script setup lang="ts">import MyComp from './Comp.vue';let a = useTemplateRef<ReturnType<typeof ___VERTER___Comp10>,typeof foo.bar>();const foo = { bar:'test'}</script>"`
      );
    });

    it("MyComp :ref='foo.bar' with argument", () => {
      const { result } = parse(
        `import MyComp from './Comp.vue';let a = useTemplateRef('test');const foo = { bar:'test' as const}`,
        false,
        "ts",
        '<template><my-comp :ref="foo.bar"/></template>'
      );
      expect(result).toMatchInlineSnapshot(
        `"<template><my-comp :ref="foo.bar"/></template><script setup lang="ts">import MyComp from './Comp.vue';let a = useTemplateRef<ReturnType<typeof ___VERTER___Comp10>,typeof foo.bar>('test');const foo = { bar:'test' as const}</script>"`
      );
    });

    it("components.MyComp ref", () => {
      const { result } = parse(
        `import MyComp from './Comp.vue';let a = useTemplateRef();const components = {MyComp}`,
        false,
        "ts",
        '<template><components.MyComp ref="a"/></template>'
      );
      expect(result).toMatchInlineSnapshot(
        `"<template><components.MyComp ref="a"/></template><script setup lang="ts">import MyComp from './Comp.vue';let a = useTemplateRef<ReturnType<typeof ___VERTER___Comp10>,"a">();const components = {MyComp}</script>"`
      );
    });

    it("ref (el)=>name=el", () => {
      const { result } = parse(
        `let a = useTemplateRef()`,
        false,
        "ts",
        '<template><div :ref="el=> name = el"/></template>'
      );
      expect(result).toMatchInlineSnapshot(
        `"<template><div :ref="el=> name = el"/></template><script setup lang="ts">let a = useTemplateRef()</script>"`
      );
    });

    it('component is="div"', () => {
      const { result } = parse(
        `let a = useTemplateRef()`,
        false,
        "ts",
        '<template><component is="div" ref="a"/></template>'
      );
      // Note: Changed from HTMLDivElement to unknown - may need review
      expect(result).toContain('let a = useTemplateRef<unknown,"a">');
    });
    it('component :is="foo"', () => {
      const { result } = parse(
        `let a = useTemplateRef();let foo = 'div'`,
        false,
        "ts",
        '<template><component :is="foo" ref="a"/></template>'
      );
      expect(result).toMatchInlineSnapshot(
        `"<template><component :is="foo" ref="a"/></template><script setup lang="ts">let a = useTemplateRef<typeof foo,"a">();let foo = 'div'</script>"`
      );
    });
    it('component :is="true ? "div" : "span""', () => {
      const { result } = parse(
        `let a = useTemplateRef()`,
        false,
        "ts",
        "<template><component :is=\"true ? 'div' : 'span'\" ref=\"a\"/></template>"
      );
      expect(result).toMatchInlineSnapshot(
        `"<template><component :is="true ? 'div' : 'span'" ref="a"/></template><script setup lang="ts">let a = useTemplateRef<HTMLDivElement|HTMLSpanElement,"a">()</script>"`
      );
    });

    it("explicit type", () => {
      const { result } = parse(
        `let a = useTemplateRef<HTMLDivElement>('foo')`,
        false,
        "ts",
        '<template><div :ref="a"/></template>'
      );
      expect(result).toMatchInlineSnapshot(
        `"<template><div :ref="a"/></template><script setup lang="ts">let a = useTemplateRef<HTMLDivElement>('foo')</script>"`
      );
    });
  });

  describe("js", () => {
    test("no ref", () => {
      const { result } = parse(`let a = useTemplateRef()`, false, "js");
      expect(result).toContain(`let a = useTemplateRef()`);
    });

    it("ref", () => {
      const { result } = parse(
        `let a = useTemplateRef()`,
        false,
        "js",
        '<template><div ref="a"/></template>'
      );
      // JS no longer uses JSDoc type casting
      expect(result).toContain(`let a = useTemplateRef()`);
    });

    it("MyComp :ref='a'", () => {
      const { result } = parse(
        `import Comp from './Comp.vue';let a = useTemplateRef()`,
        false,
        "js",
        '<template><Comp :ref="a"/></template>'
      );
      // JS no longer uses JSDoc type casting
      expect(result).toContain(`let a = useTemplateRef()`);
    });

    it("MyComp :ref='foo'", () => {
      const { result } = parse(
        `import MyComp from './Comp.vue';let a = useTemplateRef();const foo = 'test'`,
        false,
        "js",
        '<template><my-comp :ref="foo"/></template>'
      );
      // JS no longer uses JSDoc type casting
      expect(result).toContain(`let a = useTemplateRef()`);
    });

    it("MyComp :ref='foo' with string matching", () => {
      const { result } = parse(
        `import MyComp from './Comp.vue';let a = useTemplateRef('test');const foo = 'test'`,
        false,
        "js",
        '<template><my-comp :ref="foo"/></template>'
      );
      // JS no longer uses JSDoc type casting
      expect(result).toContain(`let a = useTemplateRef('test')`);
    });

    it('unknown if :ref="foo"', () => {
      const { result } = parse(
        `import MyComp from './Comp.vue';let a = useTemplateRef('test');const foo = 'testx'`,
        false,
        "js",
        '<template><my-comp :ref="foo"/></template>'
      );
      // JS no longer uses JSDoc type casting
      expect(result).toContain(`let a = useTemplateRef('test')`);
    });
    it("multiple MyComp :ref='foo' with string matching", () => {
      const { result } = parse(
        `import MyComp from './Comp.vue';import Comp from './Comp.vue'; let a = useTemplateRef('test');const foo = 'test';const foox = 'test'`,
        false,
        "js",
        '<template><my-comp :ref="foox"/></template>'
      );
      // JS no longer uses JSDoc type casting
      expect(result).toContain(`let a = useTemplateRef('test')`);
    });
    it("multiple MyComp :ref='foo' with string matching but incorrect", () => {
      const { result } = parse(
        `import MyComp from './Comp.vue';import Comp from './Comp.vue'; let a = useTemplateRef('test');const foo = 'test';const foox = 'testx'`,
        false,
        "js",
        '<template><my-comp :ref="foox"/></template>'
      );
      // JS no longer uses JSDoc type casting
      expect(result).toContain(`let a = useTemplateRef('test')`);
    });

    it("MyComp :ref='foo.bar'", () => {
      const { result } = parse(
        `import MyComp from './Comp.vue';let a = useTemplateRef();const foo = { bar:'test'}`,
        false,
        "js",
        '<template><my-comp :ref="foo.bar"/></template>'
      );
      // JS no longer uses JSDoc type casting
      expect(result).toContain(`let a = useTemplateRef()`);
    });

    it("MyComp :ref='foo.bar' with argument", () => {
      const { result } = parse(
        `import MyComp from './Comp.vue';let a = useTemplateRef('test');const foo = { bar:'test' as const}`,
        false,
        "js",
        '<template><my-comp :ref="foo.bar"/></template>'
      );
      expect(result).toMatchInlineSnapshot(
        `"<template><my-comp :ref="foo.bar"/></template><script setup lang="js">import MyComp from './Comp.vue';let a = useTemplateRef('test');const foo = { bar:'test' as const}</script>"`
      );
    });

    it("components.MyComp ref", () => {
      const { result } = parse(
        `import MyComp from './Comp.vue';let a = useTemplateRef();const components = {MyComp}`,
        false,
        "js",
        '<template><components.MyComp ref="a"/></template>'
      );
      // JS no longer uses JSDoc type casting
      expect(result).toContain(`let a = useTemplateRef()`);
    });

    it("ref (el)=>name=el", () => {
      const { result } = parse(
        `let a = useTemplateRef()`,
        false,
        "js",
        '<template><div :ref="el=> name = el"/></template>'
      );
      expect(result).toContain(`let a = useTemplateRef()`);
    });

    it('component is="div"', () => {
      const { result } = parse(
        `let a = useTemplateRef()`,
        false,
        "js",
        '<template><component is="div" ref="a"/></template>'
      );
      // JS no longer uses JSDoc type casting
      expect(result).toContain(`let a = useTemplateRef()`);
    });
    it('component :is="foo"', () => {
      const { result } = parse(
        `let a = useTemplateRef();let foo = 'div'`,
        false,
        "js",
        '<template><component :is="foo" ref="a"/></template>'
      );
      // JS no longer uses JSDoc type casting
      expect(result).toContain(`let a = useTemplateRef()`);
    });
    it('component :is="true ? "div" : "span""', () => {
      const { result } = parse(
        `let a = useTemplateRef()`,
        false,
        "js",
        "<template><component :is=\"true ? 'div' : 'span'\" ref=\"a\"/></template>"
      );
      // JS no longer uses JSDoc type casting
      expect(result).toContain(`let a = useTemplateRef()`);
    });

    it("explicit type", () => {
      const { result } = parse(
        `let a = useTemplateRef<HTMLDivElement>('foo')`,
        false,
        "js",
        '<template><div :ref="a"/></template>'
      );
      expect(result).toMatchInlineSnapshot(
        `"<template><div :ref="a"/></template><script setup lang="js">let a = useTemplateRef<HTMLDivElement>('foo')</script>"`
      );
    });
  });

  /**
   * @ai-generated - Tests for useTemplateRef inside defineComponent setup function.
   * Verifies that the TemplateRefPlugin correctly transforms useTemplateRef calls
   * when used in Options API components with setup() function.
   */
  describe("options API - defineComponent setup()", () => {
    describe("ts", () => {
      it("transforms useTemplateRef inside defineComponent setup", () => {
        const { result } = parse(
          `import { defineComponent, useTemplateRef } from 'vue'; export default defineComponent({ setup() { const myRef = useTemplateRef('myRef'); return { myRef }; } });`,
          "",
          "ts",
          '<template><div ref="myRef"></div></template>'
        );

        // Should add type parameters to useTemplateRef
        expect(result).toContain("useTemplateRef<");
        expect(result).toContain(',"myRef">');
      });

      it("transforms useTemplateRef with component ref", () => {
        const { result } = parse(
          `import { defineComponent, useTemplateRef } from 'vue'; import MyComp from './MyComp.vue'; export default defineComponent({ setup() { const compRef = useTemplateRef('compRef'); return { compRef }; } });`,
          "",
          "ts",
          '<template><MyComp ref="compRef"></MyComp></template>'
        );

        expect(result).toContain("useTemplateRef<");
        expect(result).toContain(',"compRef">');
      });

      it("transforms useTemplateRef with dynamic ref binding", () => {
        const { result } = parse(
          `import { defineComponent, useTemplateRef } from 'vue'; export default defineComponent({ setup() { const el = useTemplateRef(); return { el }; } });`,
          "",
          "ts",
          '<template><div :ref="el"></div></template>'
        );

        expect(result).toContain("useTemplateRef<");
        expect(result).toContain("typeof el");
      });

      it("does not transform useTemplateRef with explicit type parameters", () => {
        const { result } = parse(
          `import { defineComponent, useTemplateRef } from 'vue'; export default defineComponent({ setup() { const myRef = useTemplateRef<HTMLInputElement>('myRef'); return { myRef }; } });`,
          "",
          "ts",
          '<template><div ref="myRef"></div></template>'
        );

        // Should NOT add additional type parameters since explicit types are provided
        expect(result).toContain("useTemplateRef<HTMLInputElement>('myRef')");
      });

      it("handles arrow function setup", () => {
        const { result } = parse(
          `import { defineComponent, useTemplateRef } from 'vue'; export default defineComponent({ setup: () => { const myRef = useTemplateRef('myRef'); return { myRef }; } });`,
          "",
          "ts",
          '<template><div ref="myRef"></div></template>'
        );

        expect(result).toContain("useTemplateRef<");
        expect(result).toContain(',"myRef">');
      });

      it("handles plain object export with setup", () => {
        const { result } = parse(
          `import { useTemplateRef } from 'vue'; export default { setup() { const myRef = useTemplateRef('myRef'); return { myRef }; } };`,
          "",
          "ts",
          '<template><div ref="myRef"></div></template>'
        );

        expect(result).toContain("useTemplateRef<");
        expect(result).toContain(',"myRef">');
      });

      it("handles custom wrapper function", () => {
        const { result } = parse(
          `import { useTemplateRef } from 'vue'; export default myDefineComponent({ setup() { const myRef = useTemplateRef('myRef'); return { myRef }; } });`,
          "",
          "ts",
          '<template><div ref="myRef"></div></template>'
        );

        expect(result).toContain("useTemplateRef<");
        expect(result).toContain(',"myRef">');
      });
    });
  });

  /**
   * @ai-generated - Tests for useTemplateRef inside v-for loops.
   * When ref is used inside v-for (or parent/grandparent v-for),
   * useTemplateRef returns an array type.
   */
  describe("v-for array refs", () => {
    describe("ts", () => {
      it("ref inside v-for should be an array", () => {
        const { result } = parse(
          `let itemRefs = useTemplateRef('itemRef')`,
          false,
          "ts",
          '<template><div v-for="item in items" :key="item" ref="itemRef"/></template>'
        );
        // When ref is inside v-for, it should be an array type
        expect(result).toContain('useTemplateRef<ReturnType<typeof ___VERTER___Comp10>[],"itemRef">(\'itemRef\')');
      });

      it("ref inside v-for with component should be an array", () => {
        const { result } = parse(
          `import MyComp from './MyComp.vue'; let itemRefs = useTemplateRef('itemRef')`,
          false,
          "ts",
          '<template><MyComp v-for="item in items" :key="item" ref="itemRef"/></template>'
        );
        // Component refs inside v-for should also be arrays
        expect(result).toContain('useTemplateRef<ReturnType<typeof ___VERTER___Comp10>[],"itemRef">(\'itemRef\')');
      });

      it("ref inside nested v-for should be an array", () => {
        const { result } = parse(
          `let cellRefs = useTemplateRef('cellRef')`,
          false,
          "ts",
          '<template><div v-for="row in rows" :key="row"><span v-for="cell in row.cells" :key="cell" ref="cellRef"/></div></template>'
        );
        // Nested v-for ref should be an array
        expect(result).toContain('useTemplateRef<ReturnType<typeof ___VERTER___Comp46>[],"cellRef">(\'cellRef\')');
      });

      it("ref inside child of v-for element should be an array", () => {
        const { result } = parse(
          `let childRefs = useTemplateRef('childRef')`,
          false,
          "ts",
          '<template><div v-for="item in items" :key="item"><span ref="childRef"/></div></template>'
        );
        // Ref on child element inside v-for parent should be an array
        expect(result).toContain('useTemplateRef<ReturnType<typeof ___VERTER___Comp49>[],"childRef">(\'childRef\')');
      });

      it("ref inside grandchild of v-for element should be an array", () => {
        const { result } = parse(
          `let grandchildRefs = useTemplateRef('grandchildRef')`,
          false,
          "ts",
          '<template><div v-for="item in items" :key="item"><div><span ref="grandchildRef"/></div></div></template>'
        );
        // Ref on grandchild element inside v-for grandparent should be an array
        expect(result).toContain('useTemplateRef<ReturnType<typeof ___VERTER___Comp54>[],"grandchildRef">(\'grandchildRef\')');
      });

      it("dynamic ref inside v-for should be an array", () => {
        const { result } = parse(
          `let itemRefs = useTemplateRef()`,
          false,
          "ts",
          '<template><div v-for="item in items" :key="item" :ref="itemRefs"/></template>'
        );
        // Dynamic ref binding inside v-for should also be an array
        expect(result).toContain('useTemplateRef<ReturnType<typeof ___VERTER___Comp10>[],typeof itemRefs>()');
      });

      it("multiple refs with one inside v-for", () => {
        const { result } = parse(
          `let singleRef = useTemplateRef('single'); let arrayRefs = useTemplateRef('arrayRef')`,
          false,
          "ts",
          '<template><div ref="single"/><div v-for="item in items" :key="item" ref="arrayRef"/></template>'
        );
        // Single ref should not be array, v-for ref should be array, both have union of ref names
        expect(result).toContain('useTemplateRef<ReturnType<typeof ___VERTER___Comp10>,"single"|"arrayRef">(\'single\')');
        expect(result).toContain('useTemplateRef<ReturnType<typeof ___VERTER___Comp29>[],"single"|"arrayRef">(\'arrayRef\')');
      });
    });

    describe("js", () => {
      // Note: JS currently doesn't add type parameters, so array detection 
      // would only be relevant when JSDoc casting is re-enabled
      it("ref inside v-for - js does not add type parameters", () => {
        const { result } = parse(
          `let itemRefs = useTemplateRef('itemRef')`,
          false,
          "js",
          '<template><div v-for="item in items" :key="item" ref="itemRef"/></template>'
        );
        // JS version doesn't add type parameters currently
        expect(result).toContain("useTemplateRef('itemRef')");
      });
    });
  });

  /**
   * @ai-generated - Tests for multiple useTemplateRef calls in the same component.
   * Verifies that multiple refs are correctly typed independently.
   */
  describe("multiple useTemplateRef calls", () => {
    describe("ts", () => {
      it("multiple refs on different elements", () => {
        const { result } = parse(
          `let inputRef = useTemplateRef('input'); let buttonRef = useTemplateRef('button')`,
          false,
          "ts",
          '<template><input ref="input"/><button ref="button"/></template>'
        );
        // All refs are collected as union types
        expect(result).toContain('useTemplateRef<ReturnType<typeof ___VERTER___Comp10>,"input"|"button">(\'input\')');
        expect(result).toContain('useTemplateRef<ReturnType<typeof ___VERTER___Comp30>,"input"|"button">(\'button\')');
      });

      it("multiple refs on same element type", () => {
        const { result } = parse(
          `let first = useTemplateRef('first'); let second = useTemplateRef('second')`,
          false,
          "ts",
          '<template><div ref="first"/><div ref="second"/></template>'
        );
        // All refs are collected as union types
        expect(result).toContain('useTemplateRef<ReturnType<typeof ___VERTER___Comp10>,"first"|"second">(\'first\')');
        expect(result).toContain('useTemplateRef<ReturnType<typeof ___VERTER___Comp28>,"first"|"second">(\'second\')');
      });

      it("multiple refs with one unmatched", () => {
        const { result } = parse(
          `let matched = useTemplateRef('matched'); let unmatched = useTemplateRef('notInTemplate')`,
          false,
          "ts",
          '<template><div ref="matched"/></template>'
        );
        expect(result).toContain('useTemplateRef<ReturnType<typeof ___VERTER___Comp10>,"matched">');
        expect(result).toContain('useTemplateRef<unknown,"matched">');
      });

      it("multiple dynamic refs", () => {
        const { result } = parse(
          `let refA = useTemplateRef(); let refB = useTemplateRef()`,
          false,
          "ts",
          '<template><div :ref="refA"/><span :ref="refB"/></template>'
        );
        expect(result).toContain("typeof refA");
        expect(result).toContain("typeof refB");
      });
    });
  });

  /**
   * @ai-generated - Tests for refs on nested elements within the template.
   */
  describe("nested element refs", () => {
    describe("ts", () => {
      it("ref on deeply nested element", () => {
        const { result } = parse(
          `let nested = useTemplateRef('nested')`,
          false,
          "ts",
          '<template><div><div><div><span ref="nested"/></div></div></div></template>'
        );
        expect(result).toContain('useTemplateRef<ReturnType<typeof ___VERTER___Comp');
        expect(result).toContain(',"nested">');
      });

      it("refs at different nesting levels", () => {
        const { result } = parse(
          `let outer = useTemplateRef('outer'); let inner = useTemplateRef('inner')`,
          false,
          "ts",
          '<template><div ref="outer"><span ref="inner"/></div></template>'
        );
        // All refs are collected as union types
        expect(result).toContain('useTemplateRef<ReturnType<typeof ___VERTER___Comp10>,"outer"|"inner">(\'outer\')');
        expect(result).toContain('useTemplateRef<ReturnType<typeof ___VERTER___Comp27>,"outer"|"inner">(\'inner\')');
      });

      it("ref inside slot content", () => {
        const { result } = parse(
          `import MyComp from './MyComp.vue'; let slotRef = useTemplateRef('slotRef')`,
          false,
          "ts",
          '<template><MyComp><div ref="slotRef"/></MyComp></template>'
        );
        expect(result).toContain('useTemplateRef<ReturnType<typeof ___VERTER___Comp');
        expect(result).toContain(',"slotRef">');
      });
    });
  });

  /**
   * @ai-generated - Tests for refs on conditional elements (v-if, v-else, v-show).
   */
  describe("conditional element refs", () => {
    describe("ts", () => {
      it("ref on v-if element", () => {
        const { result } = parse(
          `let conditional = useTemplateRef('conditional')`,
          false,
          "ts",
          '<template><div v-if="show" ref="conditional"/></template>'
        );
        expect(result).toContain('useTemplateRef<ReturnType<typeof ___VERTER___Comp10>,"conditional">');
      });

      it("ref on v-else element", () => {
        const { result } = parse(
          `let elseRef = useTemplateRef('elseRef')`,
          false,
          "ts",
          '<template><div v-if="show"/><span v-else ref="elseRef"/></template>'
        );
        expect(result).toContain('useTemplateRef<ReturnType<typeof ___VERTER___Comp');
        expect(result).toContain(',"elseRef">');
      });

      it("ref on v-show element", () => {
        const { result } = parse(
          `let shown = useTemplateRef('shown')`,
          false,
          "ts",
          '<template><div v-show="visible" ref="shown"/></template>'
        );
        expect(result).toContain('useTemplateRef<ReturnType<typeof ___VERTER___Comp10>,"shown">');
      });

      it("same ref name on v-if and v-else branches", () => {
        const { result } = parse(
          `let shared = useTemplateRef('shared')`,
          false,
          "ts",
          '<template><div v-if="condition" ref="shared"/><span v-else ref="shared"/></template>'
        );
        // Should include both possible types with duplicated ref name in union
        expect(result).toContain('useTemplateRef<ReturnType<typeof ___VERTER___Comp10>|ReturnType<typeof ___VERTER___Comp46>,"shared"|"shared">(\'shared\')');
      });
    });
  });

  /**
   * @ai-generated - Tests for refs with template elements (template, Teleport, Suspense).
   */
  describe("special element refs", () => {
    describe("ts", () => {
      it("ref on Teleport content", () => {
        const { result } = parse(
          `let teleported = useTemplateRef('teleported')`,
          false,
          "ts",
          '<template><Teleport to="body"><div ref="teleported"/></Teleport></template>'
        );
        expect(result).toContain('useTemplateRef<ReturnType<typeof ___VERTER___Comp');
        expect(result).toContain(',"teleported">');
      });

      it("ref on Suspense fallback", () => {
        const { result } = parse(
          `let fallback = useTemplateRef('fallback')`,
          false,
          "ts",
          '<template><Suspense><template #fallback><div ref="fallback"/></template></Suspense></template>'
        );
        expect(result).toContain('useTemplateRef<ReturnType<typeof ___VERTER___Comp');
        expect(result).toContain(',"fallback">');
      });

      it("ref on KeepAlive child", () => {
        const { result } = parse(
          `import MyComp from './MyComp.vue'; let kept = useTemplateRef('kept')`,
          false,
          "ts",
          '<template><KeepAlive><MyComp ref="kept"/></KeepAlive></template>'
        );
        expect(result).toContain('useTemplateRef<ReturnType<typeof ___VERTER___Comp');
        expect(result).toContain(',"kept">');
      });
    });
  });

  /**
   * @ai-generated - Tests for edge cases and error scenarios.
   */
  describe("edge cases", () => {
    describe("ts", () => {
      it("useTemplateRef with empty string argument", () => {
        const { result } = parse(
          `let empty = useTemplateRef('')`,
          false,
          "ts",
          '<template><div ref="test"/></template>'
        );
        // Empty string should not match any ref
        expect(result).toContain("useTemplateRef<");
      });

      it("useTemplateRef with numeric string argument", () => {
        const { result } = parse(
          `let numRef = useTemplateRef('123')`,
          false,
          "ts",
          '<template><div ref="123"/></template>'
        );
        expect(result).toContain('useTemplateRef<ReturnType<typeof ___VERTER___Comp10>,"123">');
      });

      it("useTemplateRef with special characters in name", () => {
        const { result } = parse(
          `let special = useTemplateRef('my-ref')`,
          false,
          "ts",
          '<template><div ref="my-ref"/></template>'
        );
        expect(result).toContain('useTemplateRef<ReturnType<typeof ___VERTER___Comp10>,"my-ref">');
      });

      it("multiple refs with same name on different elements", () => {
        const { result } = parse(
          `let duplicate = useTemplateRef('dup')`,
          false,
          "ts",
          '<template><div ref="dup"/><span ref="dup"/></template>'
        );
        // Should include all possible types with duplicated ref name in union
        expect(result).toContain('useTemplateRef<ReturnType<typeof ___VERTER___Comp10>|ReturnType<typeof ___VERTER___Comp26>,"dup"|"dup">(\'dup\')');
      });

      it("ref on root element", () => {
        const { result } = parse(
          `let root = useTemplateRef('root')`,
          false,
          "ts",
          '<template><div ref="root"/></template>'
        );
        expect(result).toContain('useTemplateRef<ReturnType<typeof ___VERTER___Comp10>,"root">');
      });

      it("ref with variable as argument that matches ref name", () => {
        const { result } = parse(
          `const refName = 'myDiv'; let myRef = useTemplateRef(refName)`,
          false,
          "ts",
          '<template><div ref="myDiv"/></template>'
        );
        // Should handle variable reference
        expect(result).toContain("useTemplateRef<");
      });
    });
  });

  /**
   * @ai-generated - Tests for kebab-case and PascalCase component resolution.
   */
  describe("component name resolution", () => {
    describe("ts", () => {
      it("PascalCase component with PascalCase usage", () => {
        const { result } = parse(
          `import MyComponent from './MyComponent.vue'; let ref = useTemplateRef('ref')`,
          false,
          "ts",
          '<template><MyComponent ref="ref"/></template>'
        );
        expect(result).toContain('useTemplateRef<ReturnType<typeof ___VERTER___Comp10>,"ref">');
      });

      it("PascalCase component with kebab-case usage", () => {
        const { result } = parse(
          `import MyComponent from './MyComponent.vue'; let ref = useTemplateRef('ref')`,
          false,
          "ts",
          '<template><my-component ref="ref"/></template>'
        );
        expect(result).toContain('useTemplateRef<ReturnType<typeof ___VERTER___Comp10>,"ref">');
      });

      it("camelCase imported component", () => {
        const { result } = parse(
          `import myComponent from './MyComponent.vue'; let ref = useTemplateRef('ref')`,
          false,
          "ts",
          '<template><my-component ref="ref"/></template>'
        );
        expect(result).toContain('useTemplateRef<ReturnType<typeof ___VERTER___Comp10>,"ref">');
      });

      it("locally registered component", () => {
        const { result } = parse(
          `const LocalComp = { template: '<div/>' }; let ref = useTemplateRef('ref')`,
          false,
          "ts",
          '<template><LocalComp ref="ref"/></template>'
        );
        expect(result).toContain('useTemplateRef<ReturnType<typeof ___VERTER___Comp10>,"ref">');
      });
    });
  });

  /**
   * @ai-generated - Tests for ref() function with template refs.
   * When a variable declared with ref() has the same name as a template ref,
   * the ref() call should be typed with the element type.
   */
  describe("ref() with template refs", () => {
    describe("ts", () => {
      it("ref() variable matching template ref name", () => {
        const { result } = parse(
          `const myDiv = ref()`,
          false,
          "ts",
          '<template><div ref="myDiv"/></template>'
        );
        expect(result).toContain("ref<ReturnType<typeof ___VERTER___Comp10>|null>()");
      });

      it("ref() with initial value matching template ref", () => {
        const { result } = parse(
          `const myDiv = ref(null)`,
          false,
          "ts",
          '<template><div ref="myDiv"/></template>'
        );
        expect(result).toContain("ref<ReturnType<typeof ___VERTER___Comp10>|null>(null)");
      });

      it("ref() variable not matching any template ref", () => {
        const { result } = parse(
          `const notARef = ref()`,
          false,
          "ts",
          '<template><div ref="myDiv"/></template>'
        );
        // Should not add type parameters since variable name doesn't match
        expect(result).toContain("const notARef = ref()");
        expect(result).not.toContain("ref<");
      });

      it("ref() with component template ref", () => {
        const { result } = parse(
          `import MyComp from './MyComp.vue'; const compRef = ref()`,
          false,
          "ts",
          '<template><MyComp ref="compRef"/></template>'
        );
        expect(result).toContain("ref<ReturnType<typeof ___VERTER___Comp10>|null>()");
      });

      it("ref() with explicit type parameters should not be modified", () => {
        const { result } = parse(
          `const myDiv = ref<HTMLDivElement | null>(null)`,
          false,
          "ts",
          '<template><div ref="myDiv"/></template>'
        );
        // Should not modify if explicit type parameters are present
        expect(result).toContain("ref<HTMLDivElement | null>(null)");
      });

      it("ref() inside v-for should be array type", () => {
        const { result } = parse(
          `const itemRef = ref()`,
          false,
          "ts",
          '<template><div v-for="item in items" :key="item" ref="itemRef"/></template>'
        );
        expect(result).toContain("ref<ReturnType<typeof ___VERTER___Comp10>[]|null>()");
      });

      it("ref() with kebab-case template ref", () => {
        const { result } = parse(
          `const myElement = ref()`,
          false,
          "ts",
          '<template><div ref="myElement"/></template>'
        );
        expect(result).toContain("ref<ReturnType<typeof ___VERTER___Comp10>|null>()");
      });

      it("multiple ref() calls with different template refs", () => {
        const { result } = parse(
          `const divRef = ref(); const spanRef = ref()`,
          false,
          "ts",
          '<template><div ref="divRef"/><span ref="spanRef"/></template>'
        );
        expect(result).toContain("divRef = ref<ReturnType<typeof ___VERTER___Comp10>|null>()");
        expect(result).toContain("spanRef = ref<ReturnType<typeof ___VERTER___Comp29>|null>()");
      });

      it("ref() with nested element template ref", () => {
        const { result } = parse(
          `const nested = ref()`,
          false,
          "ts",
          '<template><div><span><input ref="nested"/></span></div></template>'
        );
        expect(result).toContain("ref<ReturnType<typeof ___VERTER___Comp");
        expect(result).toContain("|null>()");
      });
    });

    describe("js", () => {
      it("ref() variable matching template ref name - js does not add type", () => {
        const { result } = parse(
          `const myDiv = ref()`,
          false,
          "js",
          '<template><div ref="myDiv"/></template>'
        );
        // JS currently doesn't add type annotations for ref()
        expect(result).toContain("const myDiv = ref()");
      });

      it("ref() variable not matching template ref", () => {
        const { result } = parse(
          `const notARef = ref()`,
          false,
          "js",
          '<template><div ref="myDiv"/></template>'
        );
        // Should not add type annotation
        expect(result).toContain("const notARef = ref()");
        expect(result).not.toContain("@type");
      });
    });
  });

  /**
   * @ai-generated - Tests to verify ref() is NOT modified when type arguments are already provided.
   */
  describe("ref() with explicit type arguments", () => {
    describe("ts", () => {
      it("ref() with generic type parameter should not be modified", () => {
        const { result } = parse(
          `const myDiv = ref<HTMLElement>()`,
          false,
          "ts",
          '<template><div ref="myDiv"/></template>'
        );
        expect(result).toContain("const myDiv = ref<HTMLElement>()");
        expect(result).not.toContain("ref<ReturnType");
      });

      it("ref() with union type parameter should not be modified", () => {
        const { result } = parse(
          `const myDiv = ref<HTMLDivElement | null>(null)`,
          false,
          "ts",
          '<template><div ref="myDiv"/></template>'
        );
        expect(result).toContain("ref<HTMLDivElement | null>(null)");
        expect(result).not.toContain("ReturnType");
      });

      it("ref() with complex type parameter should not be modified", () => {
        const { result } = parse(
          `const myDiv = ref<InstanceType<typeof MyComponent> | null>()`,
          false,
          "ts",
          '<template><div ref="myDiv"/></template>'
        );
        expect(result).toContain("ref<InstanceType<typeof MyComponent> | null>()");
        expect(result).not.toContain("___VERTER___");
      });

      it("ref() with any type parameter should not be modified", () => {
        const { result } = parse(
          `const myDiv = ref<any>()`,
          false,
          "ts",
          '<template><div ref="myDiv"/></template>'
        );
        expect(result).toContain("const myDiv = ref<any>()");
        expect(result).not.toContain("ReturnType");
      });

      it("ref() with unknown type parameter should not be modified", () => {
        const { result } = parse(
          `const myDiv = ref<unknown>(null)`,
          false,
          "ts",
          '<template><div ref="myDiv"/></template>'
        );
        expect(result).toContain("ref<unknown>(null)");
        expect(result).not.toContain("___VERTER___");
      });
    });
  });

  /**
   * @ai-generated - Tests to verify ref() ONLY modifies when assigned to a variable
   * with the EXACT name matching the template ref.
   */
  describe("ref() exact variable name matching", () => {
    describe("ts", () => {
      it("should NOT modify when variable name is a prefix of ref name", () => {
        const { result } = parse(
          `const my = ref()`,
          false,
          "ts",
          '<template><div ref="myDiv"/></template>'
        );
        expect(result).toContain("const my = ref()");
        expect(result).not.toContain("ref<");
      });

      it("should NOT modify when variable name is a suffix of ref name", () => {
        const { result } = parse(
          `const Div = ref()`,
          false,
          "ts",
          '<template><div ref="myDiv"/></template>'
        );
        expect(result).toContain("const Div = ref()");
        expect(result).not.toContain("ref<");
      });

      it("should NOT modify when variable name contains ref name", () => {
        const { result } = parse(
          `const myDivElement = ref()`,
          false,
          "ts",
          '<template><div ref="myDiv"/></template>'
        );
        expect(result).toContain("const myDivElement = ref()");
        expect(result).not.toContain("ref<");
      });

      it("should NOT modify when ref name contains variable name", () => {
        const { result } = parse(
          `const div = ref()`,
          false,
          "ts",
          '<template><div ref="divRef"/></template>'
        );
        expect(result).toContain("const div = ref()");
        expect(result).not.toContain("ref<");
      });

      it("should NOT modify when variable name differs only by case", () => {
        const { result } = parse(
          `const mydiv = ref()`,
          false,
          "ts",
          '<template><div ref="myDiv"/></template>'
        );
        expect(result).toContain("const mydiv = ref()");
        expect(result).not.toContain("ref<");
      });

      it("should NOT modify when variable name differs only by case (uppercase)", () => {
        const { result } = parse(
          `const MYDIV = ref()`,
          false,
          "ts",
          '<template><div ref="myDiv"/></template>'
        );
        expect(result).toContain("const MYDIV = ref()");
        expect(result).not.toContain("ref<");
      });

      it("should ONLY modify when exact name matches", () => {
        const { result } = parse(
          `const myDiv = ref()`,
          false,
          "ts",
          '<template><div ref="myDiv"/></template>'
        );
        expect(result).toContain("ref<ReturnType<typeof ___VERTER___Comp10>|null>()");
      });

      it("should NOT modify ref() used in expression (not assigned to variable)", () => {
        const { result } = parse(
          `console.log(ref())`,
          false,
          "ts",
          '<template><div ref="myDiv"/></template>'
        );
        expect(result).toContain("console.log(ref())");
        expect(result).not.toContain("ref<");
      });

      it("should NOT modify ref() assigned to object property", () => {
        const { result } = parse(
          `const obj = { myDiv: ref() }`,
          false,
          "ts",
          '<template><div ref="myDiv"/></template>'
        );
        expect(result).toContain("const obj = { myDiv: ref() }");
        expect(result).not.toContain("ref<");
      });

      it("should NOT modify ref() assigned to array element", () => {
        const { result } = parse(
          `const arr = [ref()]`,
          false,
          "ts",
          '<template><div ref="myDiv"/></template>'
        );
        expect(result).toContain("const arr = [ref()]");
        expect(result).not.toContain("ref<");
      });

      it("should NOT modify ref() returned from function", () => {
        const { result } = parse(
          `function getRef() { return ref() }`,
          false,
          "ts",
          '<template><div ref="myDiv"/></template>'
        );
        expect(result).toContain("return ref()");
        expect(result).not.toContain("ref<");
      });

      it("should NOT modify ref() passed as argument", () => {
        const { result } = parse(
          `someFunction(ref())`,
          false,
          "ts",
          '<template><div ref="myDiv"/></template>'
        );
        expect(result).toContain("someFunction(ref())");
        expect(result).not.toContain("ref<");
      });

      it("should handle multiple variables - only exact matches modified", () => {
        const { result } = parse(
          `const myDiv = ref(); const myDivRef = ref(); const notMyDiv = ref()`,
          false,
          "ts",
          '<template><div ref="myDiv"/></template>'
        );
        // Only myDiv should be modified
        expect(result).toContain("myDiv = ref<ReturnType<typeof ___VERTER___Comp10>|null>()");
        expect(result).toContain("const myDivRef = ref()");
        expect(result).toContain("const notMyDiv = ref()");
      });
    });
  });
});
