/// <reference types="vitest/globals" />
/**
 * @ai-generated - This test file was generated with AI assistance.
 * Tests for the SFCCleanerPlugin which comments out non-template blocks
 * when processing the template block.
 * - Single line block commenting
 * - Multi-line block commenting
 * - Multiple non-template blocks
 * - Script and style block handling
 */
import { MagicString } from "@vue/compiler-sfc";
import { parser } from "../../../../parser";
import { ParsedBlockTemplate } from "../../../../parser/types";
import { processTemplate, TemplateContext } from "../../template";
import { SFCCleanerPlugin } from "./index";

describe("process template plugin sfc-cleaner", () => {
  function parse(
    content: string,
    options: Partial<TemplateContext> = {},
    pre: string = "",
    post: string = ""
  ) {
    const source = `${pre}<template>${content}</template>${post}`;
    const parsed = parser(source);

    const s = new MagicString(source);

    const templateBlock = parsed.blocks.find(
      (x) => x.type === "template"
    ) as ParsedBlockTemplate;

    const r = processTemplate(templateBlock.result.items, [SFCCleanerPlugin], {
      ...options,
      s,
      filename: "test.vue",
      blocks: parsed.blocks,
      block: templateBlock,
      blockNameResolver: (name) => name,
    });

    return r;
  }

  it("comments out script block (single line)", () => {
    const { result } = parse(
      `<div>hi</div>`,
      {},
      "<script>let a = 0;</script>"
    );
    expect(result).toContain("// <script>let a = 0;</script>");
  });

  it("comments out style block (single line)", () => {
    const { result } = parse(
      `<div>hi</div>`,
      {},
      "",
      "<style>.a { color: red }</style>"
    );
    expect(result).toContain("// <style>.a { color: red }</style>");
  });

  it("comments every line in multi-line script block", () => {
    const pre = `<script>\nlet a = 0;\nlet b = 1;\n</script>`;
    const { result } = parse(`<div>hi</div>`, {}, pre);
    expect(result).toMatchInlineSnapshot(`
      "// <script>
      // let a = 0;
      // let b = 1;
      // </script><template><div>hi</div></template>"
    `);
  });

  it("comments every line in multi-line style block", () => {
    const post = `<style>\n.a { color: red }\n.b { color: blue }\n</style>`;
    const { result } = parse(`<div>hi</div>`, {}, "", post);
    expect(result).toMatchInlineSnapshot(`
      "<template><div>hi</div></template>// <style>
      // .a { color: red }
      // .b { color: blue }
      // </style>"
    `);
  });

  it("comments out both script and style blocks", () => {
    const pre = `<script>let a = 0;</script>`;
    const post = `<style>.a { color: red }</style>`;
    const { result } = parse(`<div>hi</div>`, {}, pre, post);
    expect(result).toContain("// <script>let a = 0;</script>");
    expect(result).toContain("// <style>.a { color: red }</style>");
  });

  it("comments out multiple script and style blocks with multi-line content", () => {
    const pre = `<script>\nconst foo = 'bar';\n</script>`;
    const post = `<style>\n.container {\n  display: flex;\n}\n</style>`;
    const { result } = parse(`<div>content</div>`, {}, pre, post);
    expect(result).toMatchInlineSnapshot(`
      "// <script>
      // const foo = 'bar';
      // </script><template><div>content</div></template>// <style>
      // .container {
      //   display: flex;
      // }
      // </style>"
    `);
  });

  it("does not affect the template block content", () => {
    const pre = `<script>let a = 0;</script>`;
    const { result } = parse(`<div class="test">content</div>`, {}, pre);
    expect(result).toContain('<template><div class="test">content</div></template>');
  });

  it("handles script setup blocks", () => {
    const pre = `<script setup>\nconst count = ref(0);\n</script>`;
    const { result } = parse(`<div>{{ count }}</div>`, {}, pre);
    expect(result).toContain("// <script setup>");
    expect(result).toContain("// const count = ref(0);");
    expect(result).toContain("// </script>");
  });

  it("handles script with lang attribute", () => {
    const pre = `<script lang="ts">\nconst a: number = 0;\n</script>`;
    const { result } = parse(`<div>hi</div>`, {}, pre);
    expect(result).toContain('// <script lang="ts">');
    expect(result).toContain("// const a: number = 0;");
  });

  it("handles style with scoped attribute", () => {
    const post = `<style scoped>\n.a { color: red }\n</style>`;
    const { result } = parse(`<div>hi</div>`, {}, "", post);
    expect(result).toContain("// <style scoped>");
  });

  it("comments out empty script block", () => {
    const pre = `<script></script>`;
    const { result } = parse(`<div>hi</div>`, {}, pre);
    expect(result).toContain("// <script></script>");
  });

  it("comments out empty style block", () => {
    const post = `<style></style>`;
    const { result } = parse(`<div>hi</div>`, {}, "", post);
    expect(result).toContain("// <style></style>");
  });
});
