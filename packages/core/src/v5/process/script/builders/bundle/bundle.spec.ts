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
    });
  }

  it("work", () => {
    const { result } = parse(`let a = 0; import { a } from "b";`);
    expect(result).toMatchInlineSnapshot(`
      "import { type DefineProps as ___VERTER___DefineProps } from "vue";
      import { ___VERTER___default, ___VERTER___resolveProps, ___VERTER___resolveSlots } from "../test.vue.bundle.ts";
      export type ___VERTER___PartialUndefined<T> = {
        [P in keyof T]: undefined extends T[P] ? P : never;
      }[keyof T] extends infer U extends keyof T
        ? Omit<T, U> & Partial<Pick<T, U>>
        : T;

      type ___VERTER___ProcessProps<T> = T extends ___VERTER___DefineProps<infer U, infer BKeys>
        ? ___VERTER___PartialUndefined<U>
        : T;
      declare const Test: typeof ___VERTER___default & {new():{$props: (___VERTER___ProcessProps<ReturnType<typeof ___VERTER___resolveProps>>);$slots: ReturnType<typeof ___VERTER___resolveSlots>extends infer P? P extends P & 1 ? {} : P : never;}};export default Test;"
    `);
  });
});
