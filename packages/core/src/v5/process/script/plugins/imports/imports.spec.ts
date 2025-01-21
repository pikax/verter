import { MagicString } from "@vue/compiler-sfc";
import { parser } from "../../../../parser";
import { ParsedBlockScript } from "../../../../parser/types";
import { processScript } from "../../script";
import { ImportsPlugin } from "./imports.js";
import { MacrosPlugin } from "../macros";

describe("process script plugins imports", () => {
  function parse(
    content: string,
    wrapper: string | false = false,
    lang = "js"
  ) {
    const prepend = `<script ${
      wrapper === false ? "setup" : ""
    } lang="${lang}">`;
    const source = `${prepend}${content}</script>`;
    const parsed = parser(source);

    const s = new MagicString(source);

    const scriptBlock = parsed.blocks.find(
      (x) => x.type === "script"
    ) as ParsedBlockScript;

    const r = processScript(
      scriptBlock.result.items,
      [
        ImportsPlugin,
        MacrosPlugin,
        // clean template tag
        {
          pre: (s) => {
            s.update(0, prepend.length, "");
            s.update(source.length - "</script>".length, source.length, "");
          },
        },
      ],
      {
        s,
        filename: "test.vue",
        blocks: parsed.blocks,
        block: scriptBlock,
      }
    );

    return r;
  }

  it("should import plugin correctly", () => {
    const { s } = parse(`let a = 0; import { a } from "b";`);
    expect(s.toString()).toContain(`import { a } from "b";let a = 0;`);
  });

  it("should add imports", () => {
    const { s } = parse(`let a = defineModel();`, false, "ts");
    expect(s.toString()).toContain(
      `import { type ___VERTER___ModelToProps, type ___VERTER___UnionToIntersection, type ___VERTER___ModelToEmits } from "$verter/options.helper.ts";let a = defineModel();`
    );
  });
  it("should only generate 1 import", () => {
    const { s } = parse(
      `defineModel(); defineModel('foo); defineEmits()`,
      false,
      "ts"
    );
    expect(s.toString()).toContain(
      `import { type ___VERTER___ModelToProps, type ___VERTER___UnionToIntersection, type ___VERTER___ModelToEmits } from "$verter/options.helper.ts";defineModel(); defineModel('foo); defineEmits()`
    );
  });
});
