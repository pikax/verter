import { MagicString } from "@vue/compiler-sfc";
import { parser } from "../../../../parser";
import { ParsedBlockScript } from "../../../../parser/types";
import { processScript } from "../../script";

import { MacrosPlugin } from "../macros";
import { ScriptBlockPlugin } from "../script-block";
import { FullContextPlugin } from "./index.js";
import { BindingPlugin } from "../binding";

describe("process script plugin full context", () => {
  function _parse(
    content: string,
    wrapper: string | boolean = false,
    lang = "js",
    attributes = "",

    pre = "",
    post = ""
  ) {
    const prepend = `${pre}<script ${
      wrapper === false ? "setup" : ""
    } lang="${lang}" ${attributes}>`;
    const source = `${prepend}${content}</script>${post}`;
    const parsed = parser(source);

    const s = new MagicString(source);

    const scriptBlock = parsed.blocks.find(
      (x) => x.type === "script"
    ) as ParsedBlockScript;

    const r = processScript(
      scriptBlock.result.items,
      [FullContextPlugin, BindingPlugin, MacrosPlugin, ScriptBlockPlugin],
      {
        s,
        filename: "test.vue",
        blocks: parsed.blocks,
        isSetup: wrapper === false,
        block: scriptBlock,
        generic: parsed.generic,
        isAsync: parsed.isAsync,
      }
    );

    return r;
  }

  describe("ts", () => {
    describe("setup", () => {
      function parse(content: string, attributes = "") {
        return _parse(content, false, "ts", attributes);
      }
      it("work", () => {
        const { result } = parse("let a = 0");
        expect(result).toMatchInlineSnapshot(`"function script   (){let a = 0};function ___VERTER___FullContextFN() {let a = 0;return{a:a as typeof a}};;export type ___VERTER___FullContext=ReturnType<typeof ___VERTER___FullContextFN>;;export type ___VERTER___defineProps={};;export type ___VERTER___defineEmits={};;export type ___VERTER___defineExpose={};;export type ___VERTER___defineOptions={};;export type ___VERTER___defineModel={};;export type ___VERTER___defineSlots={};"`);
      });

      it("async", () => {
        const { result } = parse("let a = await Promise.resolve(0);");
        expect(result).toMatchInlineSnapshot(`"async function script   (){let a = await Promise.resolve(0);};async function ___VERTER___FullContextFN() {let a = await Promise.resolve(0);;return{a:a as typeof a}};;export type ___VERTER___FullContext=ReturnType<typeof ___VERTER___FullContextFN> extends Promise<infer R>?R:never;;export type ___VERTER___defineProps={};;export type ___VERTER___defineEmits={};;export type ___VERTER___defineExpose={};;export type ___VERTER___defineOptions={};;export type ___VERTER___defineModel={};;export type ___VERTER___defineSlots={};"`);
      });

      it("generic", () => {
        const { result } = parse("let a = {} as unknown as T", 'generic="T"');
        expect(result).toMatchInlineSnapshot(`"function script   <T>(){let a = {} as unknown as T};function ___VERTER___FullContextFN<T>() {let a = {} as unknown as T;return{a:a as typeof a}};;export type ___VERTER___FullContext<T>=ReturnType<typeof ___VERTER___FullContextFN<T>>;;export type ___VERTER___defineProps={};;export type ___VERTER___defineEmits={};;export type ___VERTER___defineExpose={};;export type ___VERTER___defineOptions={};;export type ___VERTER___defineModel={};;export type ___VERTER___defineSlots={};"`);
      });

      it("generic async", () => {
        const { result } = parse(
          "let a = await Promise.resolve({} as unknown as T)",
          'generic="T"'
        );
        expect(result).toMatchInlineSnapshot(`"async function script   <T>(){let a = await Promise.resolve({} as unknown as T)};async function ___VERTER___FullContextFN<T>() {let a = await Promise.resolve({} as unknown as T);return{a:a as typeof a}};;export type ___VERTER___FullContext<T>=ReturnType<typeof ___VERTER___FullContextFN<T>> extends Promise<infer R>?R:never;;export type ___VERTER___defineProps={};;export type ___VERTER___defineEmits={};;export type ___VERTER___defineExpose={};;export type ___VERTER___defineOptions={};;export type ___VERTER___defineModel={};;export type ___VERTER___defineSlots={};"`);
      });
    });
  });
});
