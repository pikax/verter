import { MagicString } from "@vue/compiler-sfc";
import { parser } from "../../../../parser";
import { ParsedBlockScript } from "../../../../parser/types";
import { processScript } from "../../script";
import { ImportsPlugin } from "./imports.js";

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
        // clean template tag
        {
          pre: (s) => {
            s.update(0, prepend.length, "");
            s.update(source.length - "</template>".length, source.length, "");
          },
        },
      ],
      {
        s,
        filename: "test.vue",
        blocks: parsed.blocks,
      }
    );

    return r;
  }

  it("should import plugin correctly", () => {
    const { s } = parse(`let a = 0; import { a } from "b"`);
    expect(s.toString()).toMatchInlineSnapshot(
      `"import { a } from 'b'; let a = 0;"`
    );
  });
});
