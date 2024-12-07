import { MagicString, parse } from "@vue/compiler-sfc";
import TemplatePlugin from "./index.js";
import { ParseScriptContext } from "../types.js";

describe("template", () => {
  it("should work", () => {
    const parsed = parse(`
    <template>
        <my-comp v-for="i in 5" :key="i" :test="i"></my-comp>
    </template>`);

    const s = new MagicString(parsed.descriptor.template.ast.source);

    const res = TemplatePlugin.process({
      sfc: parsed,
      template: parsed.descriptor.template,
      s,
    } as ParseScriptContext);

    expect(s.toString()).toMatchInlineSnapshot(`
      "
          <template>
              {___VERTER___renderList(5,i   =>{ <___VERTER___comp.MyComp  key={i} test={i}></___VERTER___comp.MyComp>})}
          </template>"
    `);
  });

  //   describe("walk", () => {});

  //   describe('walkElement', () => {
  //   })

  //   describe('walkAttribute', () => {
  //   })

  //   describe('walkExpressionNode', () => {
  //   })
});
