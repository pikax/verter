import { parser } from "../../../../parser";
import { parseTemplate } from "../../../../parser/template";
import { ParsedBlockTemplate } from "../../../../parser/types";
import { extractBlocksFromDescriptor } from "../../../../utils";
import { processTemplate } from "../../template";
import { ConditionalPlugin } from "../conditional";
import { PropPlugin } from "../prop";
import { BindingPlugin } from "./index";
import { MagicString, parse as parseSFC } from "@vue/compiler-sfc";

describe("process template plugins binding", () => {
  function parse(content: string, plugins: any[] = []) {
    const source = `<template>${content}</template>`;
    const parsed = parser(source);

    const s = new MagicString(source);

    const templateBlock = parsed.blocks.find(
      (x) => x.type === "template"
    ) as ParsedBlockTemplate;

    const r = processTemplate(
      templateBlock.result.items,
      [
        BindingPlugin,
        ...plugins,
        // clean template tag
        {
          post: (s) => {
            s.remove(0, "<template>".length);
            s.remove(source.length - "</template>".length, source.length);
          },
        },
      ],
      {
        s,
        filename: "test.vue",
        blocks: parsed.blocks,
        block: templateBlock,
        blockNameResolver: (name) => name,
      }
    );

    return r;
  }

  it("should handle binding", () => {
    const { result } = parse(`{{ test }}`);
    expect(result).toMatchInlineSnapshot(`"{{ ___VERTER___ctx.test }}"`);
  });

  test("directive", () => {
    const { result } = parse(`<div :test="test" />`);
    expect(result).toMatchInlineSnapshot(
      `"<div :test="___VERTER___ctx.test" />"`
    );
  });

  test("function", () => {
    const { result } = parse(`<div @click="test" />`);
    expect(result).toMatchInlineSnapshot(
      `"<div @click="___VERTER___ctx.test" />"`
    );
  });

  test("for", () => {
    const { result } = parse(
      `<div v-for="item in items"> {{ item + items.length}} </div>`
    );
    expect(result).toMatchInlineSnapshot(
      `"<div v-for="item in ___VERTER___ctx.items"> {{ item + ___VERTER___ctx.items.length}} </div>"`
    );
  });

  test("if", () => {
    const { result } = parse(`<div v-if="test"> test </div>`);
    expect(result).toMatchInlineSnapshot(
      `"<div v-if=\"___VERTER___ctx.test\"> test </div>"`
    );
  });

  test("args", () => {
    const { result } = parse(`<div :test="(e: Argument)=> e + test" />`);
    expect(result).toMatchInlineSnapshot(
      `"<div :test="(e: Argument)=> e + ___VERTER___ctx.test" />"`
    );
  });
  test(':[msg]="msg"', () => {
    const { result } = parse(`<div :[msg]="msg" />`);
    expect(result).toMatchInlineSnapshot(
      `"<div :[___VERTER___ctx.msg]="___VERTER___ctx.msg" />"`
    );
  });

  test(':[msg]="msg"', () => {
    const { result } = parse(`<div v-bind:[msg]="msg" />`);
    expect(result).toMatchInlineSnapshot(
      `"<div v-bind:[___VERTER___ctx.msg]="___VERTER___ctx.msg" />"`
    );
  });

  test("v-slot:[msg]", () => {
    const { result } = parse(`<div v-slot:[msg] />`);
    expect(result).toMatchInlineSnapshot(
      `"<div v-slot:[___VERTER___ctx.msg] />"`
    );
  });

  test('a href="`mailto:value`"', () => {
    const { result } = parse(`<a :href="\`mailto:\${value}\`" />`);
    expect(result).toMatchInlineSnapshot(
      `"<a :href="\`mailto:\${___VERTER___ctx.value}\`" />"`
    );
  });

  test("dynamic binding", () => {
    const { result } = parse('<Comp v-model:[`${msg}ss`]="msg" />');
    expect(result).toMatchInlineSnapshot(
      `"<Comp v-model:[\`\${___VERTER___ctx.msg}ss\`]="___VERTER___ctx.msg" />"`
    );
  });

  test("v-if + :class", () => {
    const { result } = parse('<i v-if="icon" :class="icon" />', [
      ConditionalPlugin,
      PropPlugin,
    ]);
    expect(result).toMatchInlineSnapshot(
      `"import { normalizeClass as ___VERTER___normalizeClass } from "vue";if(___VERTER___ctx.icon){<i  class={___VERTER___normalizeClass([___VERTER___ctx.icon])} />}"`
    );
  });

  describe("nested", () => {
    test("{{ { test } }}", () => {
      const { result } = parse(`{{ { test } }}`);
      expect(result).toMatchInlineSnapshot(
        `"{{ { test: ___VERTER___ctx.test } }}"`
      );
    });

    test("{{ [ test ] }}", () => {
      const { result } = parse(`{{ [ test ] }}`);
      expect(result).toMatchInlineSnapshot(`"{{ [ ___VERTER___ctx.test ] }}"`);
    });

    test("{{ ()=> { const foo = 1; } }}", () => {
      const { result } = parse(`{{ ()=> { const foo = 1; } }}`);
      expect(result).toMatchInlineSnapshot(`"{{ ()=> { const foo = 1; } }}"`);
    });

    test(`{{  const c = new Comp<'foo'>().$props['___VERTER___v-slot'];c.$props; }}`, () => {
      const { result } = parse(
        `{{  const c = new Comp<'foo'>().$props['___VERTER___v-slot']; c.$props; }}`
      );
      expect(result).toMatchInlineSnapshot(
        `"{{  const c = new ___VERTER___ctx.Comp<'foo'>().$props['___VERTER___v-slot']; c.$props; }}"`
      );
    });
  });

  describe("array", () => {
    test("array", () => {
      const { result } = parse(`{{ [test] }}`);
      expect(result).toMatchInlineSnapshot(`"{{ [___VERTER___ctx.test] }}"`);
    });

    test("array with object", () => {
      const { result } = parse(`{{ [test, { test }] }}`);
      expect(result).toMatchInlineSnapshot(
        `"{{ [___VERTER___ctx.test, { test: ___VERTER___ctx.test }] }}"`
      );
    });

    test("array with object and array", () => {
      const { result } = parse(`{{ [test, { test }, [test]] }}`);
      expect(result).toMatchInlineSnapshot(
        `"{{ [___VERTER___ctx.test, { test: ___VERTER___ctx.test }, [___VERTER___ctx.test]] }}"`
      );
    });
  });

  describe("type assertions", () => {
    test("does not prefix inside type assertions", () => {
      const { result } = parse(`{{
        () => {
          let a = {} as {
            foo: 1;
          };
          a;
        }
      }}`);
      // 'as' should NOT be prefixed with ___VERTER___ctx
      // object properties in type annotation should NOT be prefixed
      expect(result).not.toContain("___VERTER___ctx.as");
      expect(result).not.toContain("___VERTER___ctx.foo");
    });
  });

  describe("arrow function parameters", () => {
    test("does not prefix arrow function parameters", () => {
      const { result } = parse(`{{
        (foo:string)=> {
          foo.toLowerCase();
        }
      }}`);
      // 'foo' inside the arrow function body should NOT be prefixed
      // Only the first 'foo' (the parameter) might appear in output, but not prefixed
      const lines = result.split('\n');
      const bodyLine = lines.find(line => line.includes('toLowerCase'));
      if (bodyLine) {
        expect(bodyLine).not.toContain("___VERTER___ctx.foo");
      }
    });

    test("ignores arrow function parameters when binding", () => {
      const { result } = parse(`{{ (event) => { event.target } }}`);
      // 'event' parameter should not be prefixed with ___VERTER___ctx
      expect(result).not.toContain("___VERTER___ctx.event");
    });
  });
});
