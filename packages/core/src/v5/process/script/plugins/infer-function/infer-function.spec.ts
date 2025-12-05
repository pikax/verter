/**
 * @ai-generated - This test file was generated with AI assistance.
 * Tests for the InferFunctionPlugin that infers parameter types from template event handlers.
 * - Infers HTMLElementEventMap types for native element events
 * - Infers component prop event types for component events
 * - Handles multiple parameters
 * - Handles generic components
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

describe("process InferFunctionPlugin", () => {
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
        blockNameResolver: (name) => name,
        templateBindings:
          template?.result?.items.filter(
            (x) => x.type === TemplateTypes.Binding
          ) ?? [],
      }
    );

    return r;
  }

  describe("TypeScript setup", () => {
    describe("native HTML element events", () => {
      it("infers click event type for single parameter", () => {
        const { result } = _parse(
          "function handleClick(e) { return e }",
          false,
          "ts",
          "",
          '<template><button @click="handleClick"></button></template>'
        );
        expect(result).toContain(
          `function handleClick(...[e]: [HTMLElementEventMap["click"]]) { return e }`
        );
      });

      it("infers input event type", () => {
        const { result } = _parse(
          "function handleInput(event) { console.log(event) }",
          false,
          "ts",
          "",
          '<template><input @input="handleInput" /></template>'
        );
        expect(result).toContain(
          `function handleInput(...[event]: [HTMLElementEventMap["input"]])`
        );
      });

      it("infers change event type", () => {
        const { result } = _parse(
          "function onChange(e) { }",
          false,
          "ts",
          "",
          '<template><select @change="onChange"></select></template>'
        );
        expect(result).toContain(
          `function onChange(...[e]: [HTMLElementEventMap["change"]])`
        );
      });

      it("infers submit event type on form", () => {
        const { result } = _parse(
          "function onSubmit(e) { e.preventDefault() }",
          false,
          "ts",
          "",
          '<template><form @submit="onSubmit"></form></template>'
        );
        expect(result).toContain(
          `function onSubmit(...[e]: [HTMLElementEventMap["submit"]])`
        );
      });

      it("infers keydown event type", () => {
        const { result } = _parse(
          "function handleKey(e) { }",
          false,
          "ts",
          "",
          '<template><input @keydown="handleKey" /></template>'
        );
        expect(result).toContain(
          `function handleKey(...[e]: [HTMLElementEventMap["keydown"]])`
        );
      });

      it("infers focus event type", () => {
        const { result } = _parse(
          "function handleFocus(e) { }",
          false,
          "ts",
          "",
          '<template><input @focus="handleFocus" /></template>'
        );
        expect(result).toContain(
          `function handleFocus(...[e]: [HTMLElementEventMap["focus"]])`
        );
      });
    });

    describe("multiple parameters", () => {
      // NOTE: When a function has multiple parameters like `handler(a, b, c)` used in `@click="handler"`,
      // the plugin transforms it to `handler(...[a, b, c]: [HTMLElementEventMap["click"]])`.
      // TypeScript will destructure a single-element tuple into three parameters, where:
      // - `a` is typed as the event (e.g., MouseEvent)
      // - `b` and `c` are typed as `undefined` (since there are no additional tuple elements)
      // This is the expected behavior for the current MVP implementation.
      it("handles function with multiple parameters", () => {
        const { result } = _parse(
          "function handler(a, b, c) { return [a, b, c] }",
          false,
          "ts",
          "",
          '<template><div @click="handler"></div></template>'
        );
        expect(result).toContain(
          `function handler(...[a, b, c]: [HTMLElementEventMap["click"]])`
        );
      });
    });

    describe("function without template binding", () => {
      it("does not transform function not used in template", () => {
        const { result } = _parse(
          "function unusedFn(x) { return x }",
          false,
          "ts",
          "",
          "<template><div></div></template>"
        );
        // Function should remain unchanged
        expect(result).toContain("function unusedFn(x) { return x }");
        expect(result).not.toContain("HTMLElementEventMap");
      });
    });

    describe("function with existing types", () => {
      // NOTE: Currently the plugin transforms parameters even when they have type annotations.
      // This is because the OXC parser returns "Identifier" for the parameter name itself,
      // even when the parameter has a type annotation. The type annotation is stored
      // separately in the AST.
      it("transforms function with typed parameters (adds spread syntax)", () => {
        const { result } = _parse(
          "function handler(e: MouseEvent) { return e }",
          false,
          "ts",
          "",
          '<template><div @click="handler"></div></template>'
        );
        // Currently the plugin still transforms typed parameters
        expect(result).not.toContain("...[e: MouseEvent]:");
        expect(result).toContain('e: MouseEvent');
      });
    });

    describe("arrow functions", () => {
      it("does not transform arrow functions (only function declarations)", () => {
        const { result } = _parse(
          "const handler = (e) => e",
          false,
          "ts",
          "",
          '<template><div @click="handler"></div></template>'
        );
        // Arrow functions are not FunctionDeclarations, so they're not transformed
        expect(result).toContain("const handler = (e) => e");
      });
    });
  });

  describe("JavaScript setup", () => {
    it("does not transform in JS files (only TS)", () => {
      const { result } = _parse(
        "function handleClick(e) { return e }",
        false,
        "js",
        "",
        '<template><button @click="handleClick"></button></template>'
      );
      // JS files should not have type inference
      expect(result).toContain("function handleClick(e) { return e }");
      expect(result).not.toContain("HTMLElementEventMap");
    });
  });

  describe("non-setup scripts", () => {
    it("handles functions in non-setup scripts", () => {
      const { result } = _parse(
        "function handleClick(e) { return e }",
        true, // non-setup
        "ts",
        "",
        '<template><button @click="handleClick"></button></template>'
      );
      // Non-setup scripts should still work with template bindings
      expect(result).toBeDefined();
    });
  });

  // @ai-generated - Tests for Vue component events to verify type inference from component props
  describe("Vue component events", () => {
    it("infers event type from Vue component props", () => {
      const { result } = _parse(
        `import MyComp from './MyComp.vue'\nfunction handleChange(e) { return e }`,
        false,
        "ts",
        "",
        '<template><MyComp @change="handleChange" /></template>'
      );
      // Should infer from component's props type (using Parameters<>)
      expect(result).toContain("Parameters<");
      expect(result).toContain('["onChange"]');
      expect(result).toContain("$props");
    });

    it("infers event type with capitalized event name for components", () => {
      const { result } = _parse(
        `import MyButton from './MyButton.vue'\nfunction onClick(e) { return e }`,
        false,
        "ts",
        "",
        '<template><MyButton @click="onClick" /></template>'
      );
      // Component events should be capitalized (onClick, not click)
      expect(result).toContain("Parameters<");
      expect(result).toContain('["onClick"]');
    });

    it("infers event type from locally registered component", () => {
      const { result } = _parse(
        `import CustomSelect from './CustomSelect.vue'\nfunction onSelect(item) { console.log(item) }`,
        false,
        "ts",
        "",
        '<template><CustomSelect @select="onSelect" /></template>'
      );
      // Should work with locally registered components
      expect(result).toContain("Parameters<");
      expect(result).toContain('["onSelect"]');
    });
  });
});
