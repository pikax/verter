import { MagicString } from "@vue/compiler-sfc";
import { parser } from "../../../../parser";
import { ParsedBlockScript } from "../../../../parser/types";
import { processScript } from "../../script";

import { MacrosPlugin } from "../macros";
import { ScriptBlockPlugin } from "../script-block";
import { AttributesPlugin } from "./index.js";

describe("process script plugin attributes", () => {
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
      [ScriptBlockPlugin, AttributesPlugin],
      {
        s,
        filename: "test.vue",
        blocks: parsed.blocks,
        isSetup: wrapper === false,
        isAsync: parsed.isAsync,
        block: scriptBlock,
        generic: parsed.generic,
        handledAttributes: new Set(),
      }
    );

    return r;
  }

  describe("ts", () => {
    function parse(content: string, attributes = "") {
      return _parse(content, false, "ts", attributes);
    }

    it("work", () => {
      const { result } = parse(`let a = 0`, 'attributes="{ a: number }"');
      expect(result).toMatchInlineSnapshot(
        `"function script   (){let a = 0};type ___VERTER___attributes={ a: number };"`
      );
    });

    it("generic", () => {
      const { result } = parse(`let a = 0`, 'attributes="T" generic="T"');
      expect(result).toMatchInlineSnapshot(
        `"function script    <T>(){let a = 0};type ___VERTER___attributes<T>=T;"`
      );
    });

    it("async", () => {
      const { result } = parse(
        `let a = await Promise.resolve(0)`,
        'attributes="{ a: number }"'
      );
      expect(result).toMatchInlineSnapshot(
      `"async function script   (){let a = await Promise.resolve(0)};type ___VERTER___attributes={ a: number };"`);
    });
  });

  describe("js", () => {
    function parse(content: string, attributes = "") {
      return _parse(content, false, "js", attributes);
    }
    it("work", () => {
      const { result } = parse(`let a = 0`, 'attributes="{ a: number }"');
      expect(result).toMatchInlineSnapshot(`"function script   (){let a = 0}/** @typedef {{ a: number }}___VERTER___attributes*/"`);
    });
  });
});
