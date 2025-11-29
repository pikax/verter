/**
 * @ai-generated - This test file was generated with AI assistance.
 * Tests for infer-function plugin that handles function inference in SFC script.
 * Validates correct transformation of inferred function types.
 */
import { MagicString } from "@vue/compiler-sfc";
import { parser, TemplateTypes } from "../../../../parser";
import {
  ParsedBlockScript,
  ParsedBlockTemplate,
} from "../../../../parser/types";
import { processScript } from "../../script";

import { MacrosPlugin } from "../macros";
import { ScriptBlockPlugin } from "../script-block";
import { InferFunctionPlugin } from "./index.js";
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

    const template = parsed.blocks.find(
      (x) => x.type === "template"
    ) as ParsedBlockTemplate;

    const r = processScript(
      scriptBlock.result.items,
      [InferFunctionPlugin, BindingPlugin, MacrosPlugin, ScriptBlockPlugin],
      {
        s,
        filename: "test.vue",
        blocks: parsed.blocks,
        isSetup: wrapper === false,
        block: scriptBlock,
        generic: parsed.generic,
        isAsync: parsed.isAsync,
        blockNameResolver: (n) => n,
        templateBindings:
          template?.result?.items.filter(
            (x) => x.type === TemplateTypes.Binding
          ) ?? [],
      }
    );

    return r;
  }

  describe("ts", () => {
    describe("setup", () => {
      it("work", () => {
        const { result } = _parse(
          "function foo(a) { return a }",
          false,
          "ts",
          "",
          '<template><div @click="foo"></div></template>'
        );
        expect(result).toContain(
          `function foo(...[a]: [HTMLElementEventMap["click"]]) { return a }`
        );
      });
    });
  });
});
