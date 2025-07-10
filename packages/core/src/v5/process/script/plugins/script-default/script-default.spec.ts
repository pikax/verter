import { MagicString } from "@vue/compiler-sfc";
import { parser } from "../../../../parser";
import {
  ParsedBlockScript,
  ParsedBlockTemplate,
} from "../../../../parser/types";
import { processScript } from "../../script";

import { ScriptDefaultPlugin } from "./index";
import { ScriptBlockPlugin } from "../script-block";
import { BindingPlugin } from "../binding";
import { TemplateTypes } from "../../../../parser/template/types";
import { ImportsPlugin } from "../imports";
import { MacrosPlugin } from "../macros";

describe("process script plugin script-default", () => {
  function _parse(
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

    const r = processScript(
      scriptBlock.result.items,
      [
        ImportsPlugin,
        ScriptBlockPlugin,
        ScriptDefaultPlugin,
        BindingPlugin,
        MacrosPlugin,
      ],
      {
        ...parsed,
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
      }
    );
    return r;
  }

  describe("options", () => {
    test("empty options", () => {
      const { result } = _parse(``, "");

      // needs to import the defineComponent
      expect(result).toContain(
        `import { defineComponent as ___VERTER___defineComponent } from "vue"`
      );

      // needs to export the default
      expect(result).toContain(
        `;export const ___VERTER___default=___VERTER___defineComponent({});`
      );

      expect(result).not.toContain("export default");
    });

    test("export default { props: { foo: String } }", () => {
      const { result } = _parse(
        `export default { props: { foo: String } }`,
        ""
      );

      // needs to export the default
      expect(result).toContain(
        `;export const ___VERTER___default=___VERTER___defineComponent({ props: { foo: String } })`
      );
      expect(result).not.toContain("export default");
    });

    test("export default defineComponent", () => {
      const { result } = _parse(
        `export default defineComponent({ props: { foo: String } })`,
        ""
      );

      // needs to import the defineComponent
      expect(result).not.toContain("___VERTER___defineComponent");

      // needs to export the default
      expect(result).toContain(
        `export const ___VERTER___default=defineComponent({ props: { foo: String } })`
      );
      expect(result).not.toContain("export default");
    });

    test("export default customDefineComponent", () => {
      const { result } = _parse(
        `export default customDefineComponent({ setup() {} })`,
        ""
      );

      // needs to import the defineComponent
      expect(result).not.toContain("___VERTER___defineComponent");

      // needs to export the default
      expect(result).toContain(
        `export const ___VERTER___default=customDefineComponent({ setup() {} })`
      );
    });

    test("do not touch if exported variable", () => {
      const { result } = _parse(
        `const Comp = { props: { foo: String } };
export default Comp`,
        ""
      );

      // needs to import the defineComponent
      expect(result).not.toContain("___VERTER___defineComponent");

      // needs to export the default
      expect(result).toContain("const Comp = { props: { foo: String } };");
      expect(result).toContain("export const ___VERTER___default=Comp");
    });
  });

  describe.each(["js", "ts"])("lang %s", (lang) => {
    function parse(content: string, wrapper: string | false = false) {
      return _parse(content, wrapper, lang);
    }

    describe("setup", () => {
      it("empty script", () => {
        const { result } = _parse(``, false);

        // needs to import the defineComponent
        expect(result).toContain(
          `import { defineComponent as ___VERTER___defineComponent } from "vue"`
        );

        // needs to export the empty script
        expect(result).toContain(`function script  (){}`);

        // needs to export the default
        expect(result).toContain(
          `;export const ___VERTER___default=___VERTER___defineComponent({});`
        );
      });

      it("let a = 0", () => {
        const { result } = _parse(`let a = 0`, false);

        // needs to import the defineComponent
        expect(result).toContain(
          `import { defineComponent as ___VERTER___defineComponent } from "vue"`
        );

        // needs to export the empty script
        expect(result).toContain(`function script  (){let a = 0}`);

        // needs to export the default
        expect(result).toContain(
          `;export const ___VERTER___default=___VERTER___defineComponent({});`
        );
      });
      it("defineOptions({ inheritAttrs: false })", () => {
        const { result } = _parse(
          `defineOptions({ inheritAttrs: false })`,
          false
        );

        // needs to import the defineComponent
        expect(result).toContain(
          `import { defineComponent as ___VERTER___defineComponent } from "vue"`
        );

        // needs to export the empty script
        expect(result).toContain(
          `function script  (){defineOptions({ inheritAttrs: false })}`
        );

        // needs to export the default
        expect(result).toContain(
          `export const ___VERTER___default=___VERTER___defineComponent({ inheritAttrs: false });`
        );
      });

      it("let a = defineOptions({ inheritAttrs: false })", () => {
        const { result } = _parse(
          `let a = defineOptions({ inheritAttrs: false })`,
          false
        );

        // needs to import the defineComponent
        expect(result).toContain(
          `import { defineComponent as ___VERTER___defineComponent } from "vue"`
        );

        // needs to export the empty script
        expect(result).toContain(
          `function script  (){let a = defineOptions({ inheritAttrs: false })}`
        );

        // needs to export the default
        expect(result).toContain(
          `;export const ___VERTER___default=___VERTER___defineComponent({ inheritAttrs: false });`
        );
      });
    });

    describe("options", () => {
      test("empty options", () => {
        const { result } = _parse(``, "");

        // needs to import the defineComponent
        expect(result).toContain(
          `import { defineComponent as ___VERTER___defineComponent } from "vue"`
        );

        // needs to export the default
        expect(result).toContain(
          `;export const ___VERTER___default=___VERTER___defineComponent({});`
        );

        expect(result).not.toContain("export default");
      });

      test("export default { props: { foo: String } }", () => {
        const { result } = _parse(
          `export default { props: { foo: String } }`,
          ""
        );

        // needs to export the default
        expect(result).toContain(
          `;export const ___VERTER___default=___VERTER___defineComponent({ props: { foo: String } })`
        );
        expect(result).not.toContain("export default");
      });

      test("export default defineComponent", () => {
        const { result } = _parse(
          `export default defineComponent({ props: { foo: String } })`,
          ""
        );

        // needs to import the defineComponent
        expect(result).not.toContain("___VERTER___defineComponent");

        // needs to export the default
        expect(result).toContain(
          `export const ___VERTER___default=defineComponent({ props: { foo: String } })`
        );
        expect(result).not.toContain("export default");
      });

      test("export default customDefineComponent", () => {
        const { result } = _parse(
          `export default customDefineComponent({ setup() {} })`,
          ""
        );

        // needs to import the defineComponent
        expect(result).not.toContain("___VERTER___defineComponent");

        // needs to export the default
        expect(result).toContain(
          `export const ___VERTER___default=customDefineComponent({ setup() {} })`
        );
      });

      test("do not touch if exported variable", () => {
        const { result } = _parse(
          `const Comp = { props: { foo: String } };
  export default Comp`,
          ""
        );

        // needs to import the defineComponent
        expect(result).not.toContain("___VERTER___defineComponent");

        // needs to export the default
        expect(result).toContain("const Comp = { props: { foo: String } };");
        expect(result).toContain("export const ___VERTER___default=Comp");
      });

      it("export const CompB = defineComponent({ setup() {} })", () => {
        const { result } = parse(
          `export const CompB = defineComponent({ setup() {} });
export default { components: { CompB }}`,
          ""
        );

        expect(result).toContain(
          "export const CompB = defineComponent({ setup() {} });"
        );

        expect(result).toContain(
          `export const ___VERTER___default=___VERTER___defineComponent({ components: { CompB }});`
        );
      });
    });
  });
});
