/**
 * @ai-generated - This test file was generated with AI assistance.
 * Tests CurrentInstancePlugin:
 * - Generates CurrentComponentInstance type when getCurrentInstance() is used
 * - Does NOT generate when getCurrentInstance() is not used
 * - Properties derive from InstanceType<typeof Self>
 * - Handles assignment patterns (const instance = getCurrentInstance())
 * - Handles bare calls (getCurrentInstance())
 */
import { MagicString } from "@vue/compiler-sfc";
import { parser } from "../../../../parser";
import { ParsedBlockScript } from "../../../../parser/types";
import { processScript } from "../../script";

import { CurrentInstancePlugin } from "./index.js";
import { ScriptBlockPlugin } from "../script-block";
import { BindingPlugin } from "../binding";
import { TemplateBindingPlugin } from "../template-binding";
import { MacrosPlugin } from "../macros";
import { ComponentInstancePlugin } from "../component-instance";

describe("process CurrentInstancePlugin", () => {
  function parse(
    content: string,
    lang = "ts",
    attrs = "",
  ) {
    const prepend = `<script setup lang="${lang}"${attrs ? ` ${attrs}` : ""}>`;
    const source = `${prepend}${content}</script>`;
    const parsed = parser(source);

    const s = new MagicString(source);

    const scriptBlock = parsed.blocks.find((x) => x.type === "script") as ParsedBlockScript;

    const r = processScript(
      scriptBlock.result.items,
      [
        MacrosPlugin,
        TemplateBindingPlugin,
        ScriptBlockPlugin,
        BindingPlugin,
        ComponentInstancePlugin,
        CurrentInstancePlugin,
      ],
      {
        s,
        filename: "test.vue",
        blocks: parsed.blocks,
        isSetup: true,
        block: scriptBlock,
        generic: parsed.generic,
        blockNameResolver: (name) => name,
      },
    );

    return r;
  }

  describe("when getCurrentInstance() is used", () => {
    it("generates CurrentComponentInstance type for assigned call", () => {
      const { result } = parse(`const instance = getCurrentInstance()`);

      expect(result).toContain("type ___VERTER___CurrentComponentInstance");
      expect(result).toContain("InstanceType<typeof ___VERTER___Self>");
    });

    it("generates CurrentComponentInstance type for bare call", () => {
      const { result } = parse(`getCurrentInstance()`);

      expect(result).toContain("type ___VERTER___CurrentComponentInstance");
    });

    it("includes props derived from Instance", () => {
      const { result } = parse(`const instance = getCurrentInstance()`);

      expect(result).toContain("props:");
      expect(result).toContain("['$props']");
    });

    it("includes emit derived from Instance", () => {
      const { result } = parse(`const instance = getCurrentInstance()`);

      expect(result).toContain("emit:");
      expect(result).toContain("['$emit']");
    });

    it("includes slots derived from Instance", () => {
      const { result } = parse(`const instance = getCurrentInstance()`);

      expect(result).toContain("slots:");
      expect(result).toContain("['$slots']");
    });

    it("includes attrs derived from Instance", () => {
      const { result } = parse(`const instance = getCurrentInstance()`);

      expect(result).toContain("attrs:");
      expect(result).toContain("['$attrs']");
    });

    it("includes ref from getRootComponent", () => {
      const { result } = parse(`const instance = getCurrentInstance()`);

      expect(result).toContain("ref:");
      expect(result).toContain("___VERTER___getRootComponent");
    });
  });

  describe("when getCurrentInstance() is NOT used", () => {
    it("does not generate CurrentComponentInstance for regular code", () => {
      const { result } = parse(`const foo = 1`);

      // Negative: no CurrentComponentInstance type
      expect(result).not.toContain("CurrentComponentInstance");
    });

    it("does not generate CurrentComponentInstance for other function calls", () => {
      const { result } = parse(`const foo = someOtherFunction()`);

      // Negative: no CurrentComponentInstance type
      expect(result).not.toContain("CurrentComponentInstance");
    });

    it("does not generate for partial name match", () => {
      const { result } = parse(`const foo = getComponentInstance()`);

      // Negative: must be exact getCurrentInstance
      expect(result).not.toContain("CurrentComponentInstance");
    });
  });

  describe("generic components", () => {
    it("includes generics in CurrentComponentInstance", () => {
      const { result } = parse(
        `const instance = getCurrentInstance()`,
        "ts",
        `generic="T"`,
      );

      expect(result).toContain("type ___VERTER___CurrentComponentInstance");
    });
  });
});
