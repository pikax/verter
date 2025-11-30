import { MagicString } from "@vue/compiler-sfc";
import { parser } from "../../../../parser/parser.js";
import { buildBundle } from "./index.js";
import { ParsedBlockScript } from "../../../../parser/types.js";
import { processScript } from "../../script.js";

describe("process script builders bundle", () => {
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

    return buildBundle(scriptBlock.result.items, {
      isAsync: scriptBlock.result.isAsync,
      s,
      filename: "test.vue",
      blocks: parsed.blocks,
      block: scriptBlock,
      blockNameResolver: (name) => name,
    });
  }

  it("work", () => {
    const { result } = parse(`let a = 0; import { a } from "b";`);
    // Check key parts of the output
    expect(result).toContain(`import { ___VERTER___default_Component`);
    expect(result).toContain(`declare const Test: typeof ___VERTER___default_Component`);
    expect(result).toContain(`export default Test;`);
  });
});
