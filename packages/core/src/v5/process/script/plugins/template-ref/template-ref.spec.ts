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
        `"<template><div ref="a"/></template><script setup lang="ts">let a = useTemplateRef<HTMLDivElement,"a">()</script>"`
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
        `"<template><Comp :ref="a"/></template><script setup lang="ts">import Comp from './Comp.vue';let a = useTemplateRef<___VERTER___NormalisedComponents["Comp"],typeof a>()</script>"`
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
        `"<template><my-comp :ref="foo"/></template><script setup lang="ts">import MyComp from './Comp.vue';let a = useTemplateRef<___VERTER___NormalisedComponents["MyComp"],typeof foo>();const foo = 'test'</script>"`
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
        `"<template><my-comp :ref="foo"/></template><script setup lang="ts">import MyComp from './Comp.vue';let a = useTemplateRef<___VERTER___NormalisedComponents["MyComp"],typeof foo>('test');const foo = 'test'</script>"`
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
        `"<template><my-comp :ref="foox"/></template><script setup lang="ts">import MyComp from './Comp.vue';import Comp from './Comp.vue'; let a = useTemplateRef<___VERTER___NormalisedComponents["MyComp"],typeof foox>('test');const foo = 'test';const foox = 'test'</script>"`
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
        `"<template><my-comp :ref="foo.bar"/></template><script setup lang="ts">import MyComp from './Comp.vue';let a = useTemplateRef<___VERTER___NormalisedComponents["MyComp"],typeof foo.bar>();const foo = { bar:'test'}</script>"`
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
        `"<template><my-comp :ref="foo.bar"/></template><script setup lang="ts">import MyComp from './Comp.vue';let a = useTemplateRef<___VERTER___NormalisedComponents["MyComp"],typeof foo.bar>('test');const foo = { bar:'test' as const}</script>"`
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
        `"<template><components.MyComp ref="a"/></template><script setup lang="ts">import MyComp from './Comp.vue';let a = useTemplateRef<___VERTER___NormalisedComponents["components"]["MyComp"],"a">();const components = {MyComp}</script>"`
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
      expect(result).toMatchInlineSnapshot(
        `"<template><component is="div" ref="a"/></template><script setup lang="ts">let a = useTemplateRef<HTMLDivElement,"a">()</script>"`
      );
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
      expect(result).toMatchInlineSnapshot(
        `"<script setup lang="js">let a = useTemplateRef()</script>"`
      );
    });

    it("ref", () => {
      const { result } = parse(
        `let a = useTemplateRef()`,
        false,
        "js",
        '<template><div ref="a"/></template>'
      );
      expect(result).toMatchInlineSnapshot(
        `"<template><div ref="a"/></template><script setup lang="js">let a = /**@type{typeof import('vue').useTemplateRef<HTMLDivElement,"a">}*/(useTemplateRef)()</script>"`
      );
    });

    it("MyComp :ref='a'", () => {
      const { result } = parse(
        `import Comp from './Comp.vue';let a = useTemplateRef()`,
        false,
        "js",
        '<template><Comp :ref="a"/></template>'
      );
      expect(result).toMatchInlineSnapshot(
        `"<template><Comp :ref="a"/></template><script setup lang="js">import Comp from './Comp.vue';let a = /**@type{typeof import('vue').useTemplateRef<___VERTER___NormalisedComponents["Comp"],typeof a>}*/(useTemplateRef)()</script>"`
      );
    });

    it("MyComp :ref='foo'", () => {
      const { result } = parse(
        `import MyComp from './Comp.vue';let a = useTemplateRef();const foo = 'test'`,
        false,
        "js",
        '<template><my-comp :ref="foo"/></template>'
      );
      expect(result).toMatchInlineSnapshot(
        `"<template><my-comp :ref="foo"/></template><script setup lang="js">import MyComp from './Comp.vue';let a = /**@type{typeof import('vue').useTemplateRef<___VERTER___NormalisedComponents["MyComp"],typeof foo>}*/(useTemplateRef)();const foo = 'test'</script>"`
      );
    });

    it("MyComp :ref='foo' with string matching", () => {
      const { result } = parse(
        `import MyComp from './Comp.vue';let a = useTemplateRef('test');const foo = 'test'`,
        false,
        "js",
        '<template><my-comp :ref="foo"/></template>'
      );
      expect(result).toMatchInlineSnapshot(
        `"<template><my-comp :ref="foo"/></template><script setup lang="js">import MyComp from './Comp.vue';let a = /**@type{typeof import('vue').useTemplateRef<___VERTER___NormalisedComponents["MyComp"],typeof foo>}*/(useTemplateRef)('test');const foo = 'test'</script>"`
      );
    });

    it('unknown if :ref="foo"', () => {
      const { result } = parse(
        `import MyComp from './Comp.vue';let a = useTemplateRef('test');const foo = 'testx'`,
        false,
        "js",
        '<template><my-comp :ref="foo"/></template>'
      );
      expect(result).toMatchInlineSnapshot(
        `"<template><my-comp :ref="foo"/></template><script setup lang="js">import MyComp from './Comp.vue';let a = /**@type{typeof import('vue').useTemplateRef<unknown,typeof foo>}*/(useTemplateRef)('test');const foo = 'testx'</script>"`
      );
    });
    it("multiple MyComp :ref='foo' with string matching", () => {
      const { result } = parse(
        `import MyComp from './Comp.vue';import Comp from './Comp.vue'; let a = useTemplateRef('test');const foo = 'test';const foox = 'test'`,
        false,
        "js",
        '<template><my-comp :ref="foox"/></template>'
      );
      expect(result).toMatchInlineSnapshot(
        `"<template><my-comp :ref="foox"/></template><script setup lang="js">import MyComp from './Comp.vue';import Comp from './Comp.vue'; let a = /**@type{typeof import('vue').useTemplateRef<___VERTER___NormalisedComponents["MyComp"],typeof foox>}*/(useTemplateRef)('test');const foo = 'test';const foox = 'test'</script>"`
      );
    });
    it("multiple MyComp :ref='foo' with string matching but incorrect", () => {
      const { result } = parse(
        `import MyComp from './Comp.vue';import Comp from './Comp.vue'; let a = useTemplateRef('test');const foo = 'test';const foox = 'testx'`,
        false,
        "js",
        '<template><my-comp :ref="foox"/></template>'
      );
      expect(result).toMatchInlineSnapshot(
        `"<template><my-comp :ref="foox"/></template><script setup lang="js">import MyComp from './Comp.vue';import Comp from './Comp.vue'; let a = /**@type{typeof import('vue').useTemplateRef<unknown,typeof foox>}*/(useTemplateRef)('test');const foo = 'test';const foox = 'testx'</script>"`
      );
    });

    it("MyComp :ref='foo.bar'", () => {
      const { result } = parse(
        `import MyComp from './Comp.vue';let a = useTemplateRef();const foo = { bar:'test'}`,
        false,
        "js",
        '<template><my-comp :ref="foo.bar"/></template>'
      );
      expect(result).toMatchInlineSnapshot(
        `"<template><my-comp :ref="foo.bar"/></template><script setup lang="js">import MyComp from './Comp.vue';let a = /**@type{typeof import('vue').useTemplateRef<___VERTER___NormalisedComponents["MyComp"],typeof foo.bar>}*/(useTemplateRef)();const foo = { bar:'test'}</script>"`
      );
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
      expect(result).toMatchInlineSnapshot(
        `"<template><components.MyComp ref="a"/></template><script setup lang="js">import MyComp from './Comp.vue';let a = /**@type{typeof import('vue').useTemplateRef<___VERTER___NormalisedComponents["components"]["MyComp"],"a">}*/(useTemplateRef)();const components = {MyComp}</script>"`
      );
    });

    it("ref (el)=>name=el", () => {
      const { result } = parse(
        `let a = useTemplateRef()`,
        false,
        "js",
        '<template><div :ref="el=> name = el"/></template>'
      );
      expect(result).toMatchInlineSnapshot(
        `"<template><div :ref="el=> name = el"/></template><script setup lang="js">let a = useTemplateRef()</script>"`
      );
    });

    it('component is="div"', () => {
      const { result } = parse(
        `let a = useTemplateRef()`,
        false,
        "js",
        '<template><component is="div" ref="a"/></template>'
      );
      expect(result).toMatchInlineSnapshot(
        `"<template><component is="div" ref="a"/></template><script setup lang="js">let a = /**@type{typeof import('vue').useTemplateRef<HTMLDivElement,"a">}*/(useTemplateRef)()</script>"`
      );
    });
    it('component :is="foo"', () => {
      const { result } = parse(
        `let a = useTemplateRef();let foo = 'div'`,
        false,
        "js",
        '<template><component :is="foo" ref="a"/></template>'
      );
      expect(result).toMatchInlineSnapshot(
        `"<template><component :is="foo" ref="a"/></template><script setup lang="js">let a = /**@type{typeof import('vue').useTemplateRef<typeof foo,"a">}*/(useTemplateRef)();let foo = 'div'</script>"`
      );
    });
    it('component :is="true ? "div" : "span""', () => {
      const { result } = parse(
        `let a = useTemplateRef()`,
        false,
        "js",
        "<template><component :is=\"true ? 'div' : 'span'\" ref=\"a\"/></template>"
      );
      expect(result).toMatchInlineSnapshot(
        `"<template><component :is="true ? 'div' : 'span'" ref="a"/></template><script setup lang="js">let a = /**@type{typeof import('vue').useTemplateRef<HTMLDivElement|HTMLSpanElement,"a">}*/(useTemplateRef)()</script>"`
      );
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
});
