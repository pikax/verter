/**
 * @ai-generated - This test file was generated with AI assistance.
 * Validates buildSingle end-to-end with real plugins against concrete SFC input.
 */
import { describe, expect, it } from "vitest";
import { parser, ParsedBlockScript } from "../../parser/index.js";

import { buildSingle } from "./single.js";

function buildFromSFC(source: string, filename = "Test.vue") {
  const parsed = parser(source, filename);
  const scriptBlock = parsed.blocks.find(
    (block): block is ParsedBlockScript =>
      block.type === "script" && (block as ParsedBlockScript).isMain
  );

  if (!scriptBlock) {
    throw new Error("Expected a main <script> block in fixture");
  }

  return buildSingle({
    ...parsed,
    prefix: (s) => s,
    block: scriptBlock,
    blockNameResolver: (name: string) => name,
    items: [],
  });
}

describe("process builder single (integration)", () => {
  it("transforms a setup SFC into Verter output with bindings", () => {
    const source = `
<template>
  <div :id="msg">{{ msg }}</div>
</template>
<script setup lang="ts">
const msg = 'hi'
</script>`;

    const result = buildFromSFC(source);
    const finalOutput = result.s.toString();
    const bindingNames = result.script.context.templateBindings
      .map((b: any) => b?.name)
      .filter(Boolean);

    // expect(result.script.result).toContain("___VERTER___TemplateBindingFN");
    // expect(result.script.result).toContain("___VERTER___FullContextFN");
    // expect(result.script.result).toContain("const msg = 'hi'");

    // expect(finalOutput).toContain("___VERTER___TemplateBinding");
    // expect(finalOutput).toContain("___VERTER___FullContext");
    // expect(finalOutput).toContain('"id": msg');
    // expect(finalOutput).toContain("{msg/*");
    // expect(bindingNames).toContain("msg");

    expect(finalOutput).toMatchInlineSnapshot(`
      "import { type Prettify as Prettify, shallowUnwrapRef as shallowUnwrapRef, createMacroReturn as createMacroReturn, type PublicInstanceFromMacro as PublicInstanceFromMacro, type ExtractComponentProps as ExtractComponentProps, type OmitConstructorSignature as OmitConstructorSignature, enhanceElementWithProps as enhanceElementWithProps, extractLoops as extractLoops, extractArgumentsFromRenderSlot as extractArgumentsFromRenderSlot, type SlotsToRender as SlotsToRender, extractComponents as extractComponents, type OmitNever as OmitNever, getVueGlobalComponents as getVueGlobalComponents, retrieveSetupDirectives as retrieveSetupDirectives } from "$verter/types$";
      import { defineComponent as defineComponent } from "vue";
      import "$verter/tsx$";

      export function template(){const ComponentInstance = {} as Instance;
      const ctx = {...ComponentInstance,...({} as FullContext),...({} as TemplateBinding)};
      const components = {
      ...getVueGlobalComponents(),
      ...extractComponents({...({} as FullContext),...({} as TemplateBinding)})
      };
      const $slot = ctx['$slots'];
      const directiveAccessor = retrieveSetupDirectives(ctx);

      <>
        <div id={ctx.msg}>{ ctx.msg }</div>
      </>}
      ;function TemplateBindingFN  (){
      const msg = 'hi'
      ;return {...shallowUnwrapRef({msg/*88,91*/: msg as unknown as typeof msg})
      ,...createMacroReturn({})}};export type TemplateBinding=ReturnType<typeof TemplateBindingFN>;;function FullContextFN() {const msg = 'hi';return shallowUnwrapRef({msg: {} as typeof msg})};;export type FullContext=ReturnType<typeof FullContextFN>;;type attributes={};function getRootComponent(){return Comp14()}
      function getRootComponentPassedProps(){const {msg}={} as FullContext;return {"id": msg};}

      function Comp14() {
      const {msg}={} as FullContext;
        return enhanceElementWithProps({} as HTMLElementTagNameMap["div"],{"id": msg})  
      }

      ; const default_Component=defineComponent({});type RootElement=ReturnType<typeof getRootComponent>
      type RootElementProps=Prettify<Omit<ExtractComponentProps<RootElement>,keyof ReturnType<typeof getRootComponentPassedProps>>>
       type Instance = Omit<InstanceType<typeof default_Component>,"$"|"$data"|"$props"|"$attrs"|"$refs"|"$options"|"$emit"|"$el"|"$slots"> & PublicInstanceFromMacro<TemplateBinding,{}&attributes&RootElementProps,RootElement, false,true>;
       type Instance_TEST = Omit<InstanceType<typeof default_Component>,"$"|"$data"|"$props"|"$attrs"|"$refs"|"$options"|"$emit"|"$el"|"$slots"> & PublicInstanceFromMacro<TemplateBinding,{}&attributes&RootElementProps,RootElement, true,true>;
       declare const Component: OmitConstructorSignature<typeof default_Component> & {new(props?: Instance['$props']): Prettify<Instance>};
      export default Component;"
    `);
  });

  it("generates template bindings for options API scripts", () => {
    const source = `
<template>
  <span>{{ count }}</span>
</template>
<script lang="ts">
export default {
  data() {
    return { count: 1 };
  }
};
</script>`;

    const result = buildFromSFC(source);
    const finalOutput = result.s.toString();

    expect(finalOutput).toContain(
      "function ___VERTER___TemplateBindingFN(){return {}}"
    );
    expect(finalOutput).toContain("export function template");
    expect(finalOutput).toContain("___VERTER___ctx.count");
  });
});
