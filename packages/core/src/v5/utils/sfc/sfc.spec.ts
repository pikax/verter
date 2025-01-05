import {
  extractBlocksFromDescriptor,
  catchEmptyBlocks,
  findBlockLanguage,
  keepBlocks,
  removeBlockTag,
} from "./";
import { MagicString, parse } from "@vue/compiler-sfc";

describe("Utils SFC", () => {
  describe("catchEmptyBlocks", () => {
    function doExtract(source: string) {
      const { descriptor } = parse(source);
      return catchEmptyBlocks(descriptor);
    }

    it("should work", () => {
      const source = `<script></script>\n`;
      const blocks = doExtract(source);

      expect(blocks).toHaveLength(1);
      expect(blocks).toMatchObject([
        {
          type: "empty",
          attrs: {},
          content: "",
          loc: {
            source,
            start: {
              offset: 8,
            },
            end: {
              offset: 8,
            },
          },
        },
      ]);
    });

    it("with content", () => {
      const source = `<script>    </script>`;
      const blocks = doExtract(source);

      expect(blocks).toHaveLength(1);
      expect(blocks).toMatchObject([
        {
          type: "empty",
          attrs: {},
          content: "    ",
          loc: {
            source,
            start: {
              offset: 8,
            },
            end: {
              offset: 12,
            },
          },
        },
      ]);
    });

    it("should not work if has content", () => {
      const source = `<script> Hey </script>`;
      const blocks = doExtract(source);

      expect(blocks).toHaveLength(0);
      expect(blocks).toEqual([]);
    });

    it("should not catch commented", () => {
      const source = `<!-- <script></script> -->`;

      const blocks = doExtract(source);

      expect(blocks).toHaveLength(0);
      expect(blocks).toEqual([]);
    });

    it("commented + valid", () => {
      const source = `<script></script>
            <!-- <script></script> -->`;

      const blocks = doExtract(source);

      expect(blocks).toHaveLength(1);
      expect(blocks).toMatchObject([
        {
          type: "empty",
          attrs: {},
          content: "",
          loc: {
            source,
            start: {
              offset: 8,
            },
            end: {
              offset: 8,
            },
          },
        },
      ]);
    });

    it("valid sandwitched between comments", () => {
      const source = `<!-- <script></script> -->
            <script></script>
            <!-- <script></script> -->`;

      const blocks = doExtract(source);

      expect(blocks).toHaveLength(1);
      expect(blocks).toMatchObject([
        {
          type: "empty",
          attrs: {},
          content: "",
          loc: {
            source,
            start: {
              offset: 47,
            },
            end: {
              offset: 47,
            },
          },
        },
      ]);
    });

    it("valid component", () => {
      const source = `<script lang="ts">
              import { ref } from 'vue'
              
              
              const hh = 'hh';
              
              const eee= ref();
              
              </script> 
              <template>
              <div>
                  <div aria-autocomplete='' :about="false"></div>
                  <span ></span>
                  <image></image>
                  </div>
              </template>`;

      const result = doExtract(source);
      expect(result).toHaveLength(0);
    });
  });

  describe("extractBlocksFromDescriptor", () => {
    function doExtract(source: string) {
      const { descriptor, ...rest } = parse(source, {
        ignoreEmpty: false,
        templateParseOptions: { parseMode: "sfc" },
      });

      console.log("ddd", rest);
      return extractBlocksFromDescriptor(descriptor);
    }

    it("should parse", () => {
      const result = doExtract(`<script>
            const test = '';
            </script>`);

      expect(result).toMatchObject([
        {
          block: {
            type: "script",
          },
          tag: {
            type: "script",
            content: "",
            pos: {
              open: { start: 0, end: 8 },
              close: {
                start: 50,
                end: 59,
              },
              content: {
                start: 7,
                end: 7,
              },
            },
          },
        },
      ]);
    });

    it("should parse with template", () => {
      const source = `<script>
            const test = '';
            </script>
            <template> 
                <div>1</div>
            </template>`;

      const result = doExtract(source);

      expect(result).toMatchObject([
        {
          block: {
            type: "script",
          },
          tag: {
            type: "script",
            content: "",
            pos: {
              open: { start: 0, end: 8 },
              close: {
                start: 50,
                end: 59,
              },
              content: {
                start: 7,
                end: 7,
              },
            },
          },
        },
        {
          block: {
            type: "template",
          },
          tag: {
            type: "template",
            content: "",
            pos: {
              open: { start: 72, end: 82 },
              close: {
                start: 125,
                end: 136,
              },
              content: {
                start: 81,
                end: 81,
              },
            },
          },
        },
      ]);
    });

    // vue SFC does not provide empty blocks
    it("should parse empty", () => {
      const source = `<script>
            </script>
            <template>
                <div>1</div>
            </template>
            `;
      const result = doExtract(source);

      expect(result).toMatchObject([
        {
          block: {
            type: "script",
          },
          tag: {
            type: "script",
            content: "",
            pos: {
              open: { start: 0, end: 8 },
              close: {
                start: 21,
                end: 30,
              },
              content: {
                start: 7,
                end: 7,
              },
            },
          },
        },
        {
          block: {
            type: "template",
          },
          tag: {
            type: "template",
            content: "",
            pos: {
              open: { start: 43, end: 53 },
              close: {
                start: 95,
                end: 106,
              },
              content: {
                start: 52,
                end: 52,
              },
            },
          },
        },
      ]);
    });

    it("with commented block", () => {
      const source = `<script>
            const test = '';
            </script>
            <!--<script setup></script>-->
            <template> 
                <div>1</div>
            </template>`;

      const result = doExtract(source);

      expect(result).toMatchObject([
        {
          block: {
            type: "script",
            loc: {
              start: {
                offset: 8,
              },
              end: {
                offset: 50,
              },
            },
          },
          tag: {
            type: "script",
            content: "",
            pos: {
              open: { start: 0, end: 8 },
              close: {
                start: 50,
                end: 59,
              },
              content: {
                start: 7,
                end: 7,
              },
            },
          },
        },
        {
          block: {
            type: "template",
            loc: {
              start: {
                offset: 125,
              },
              end: {
                offset: 168,
              },
            },
          },
          tag: {
            type: "template",
            content: "",
            pos: {
              open: { start: 115, end: 125 },
              close: {
                start: 168,
                end: 179,
              },
              content: {
                start: 124,
                end: 124,
              },
            },
          },
        },
      ]);
    });

    it("with many commented blocks", () => {
      const source = `
            <!--This is a comment-->
            <script>
            const test = '';
            </script>
            <!--<script setup></script>-->
            <!--Another comment-->
            <!--<script setup></script>-->
            <!--<script setup></script>-->
            <!--<script setup></script>-->
            <template> 
                <div>1</div>
            </template>
            <!--<script setup></script>-->
            `;

      const result = doExtract(source);

      expect(result).toMatchObject([
        {
          block: {
            type: "script",
            loc: {
              start: {
                offset: 58,
              },
              end: {
                offset: 100,
              },
            },
          },
          tag: {
            type: "script",
            content: "",
            pos: {
              open: { start: 50, end: 58 },
              close: {
                start: 100,
                end: 109,
              },
              content: {
                start: 57,
                end: 57,
              },
            },
          },
        },
        {
          block: {
            type: "template",
          },
          tag: {
            type: "template",
            content: "",
            pos: {
              open: { start: 329, end: 339 },
              close: {
                start: 382,
                end: 393,
              },
              content: {
                start: 338,
                end: 338,
              },
            },
          },
        },
      ]);
    });

    it("with script commented", () => {
      const source = `<!-- <script commented> </script> --> <script>
foo
</script>`;

      const result = doExtract(source);

      expect(result).toMatchObject([
        {
          block: {
            type: "script",
            attrs: {},
            content: `\nfoo\n`,
          },
          tag: {
            type: "script",
            content: "",
          },
        },
      ]);

      const script = result[0];

      expect(
        source.slice(script.tag.pos.open.start, script.tag.pos.open.end)
      ).toBe(`<script>`);

      // expect(result).toBe(1);
    });

    it("block inside comment", () => {
      const source = `<!--<template></template>-->`;
      const result = doExtract(source);

      expect(result).toHaveLength(0);
    });

    it("with complex generic", () => {
      const source = `<script lang="ts" generic="T extends string & { supa?: () => number } = 'foo'">
            </script>
            <template>
            </template>
            `;

      const result = doExtract(source);

      expect(result).toHaveLength(2);

      expect(result).toMatchObject([
        {
          block: {
            type: "script",
            attrs: {
              generic: "T extends string & { supa?: () => number } = 'foo'",
              lang: "ts",
            },
            lang: "ts",
            loc: {
              start: {
                offset: 79,
              },
              end: {
                offset: 92,
              },
            },
          },
          tag: {
            type: "script",
            content:
              ' lang="ts" generic="T extends string & { supa?: () => number } = \'foo\'"',
            pos: {
              open: { start: 0, end: 79 },
              close: {
                start: 92,
                end: 101,
              },
              content: {
                start: 7,
                end: 78,
              },
            },
          },
        },
        {
          block: {
            type: "template",
          },
          tag: {
            type: "template",
          },
        },
      ]);

      const script = result[0];
      const template = result[1];

      expect(
        source.slice(script.tag.pos.open.start, script.tag.pos.open.end)
      ).toBe(
        `<script lang="ts" generic="T extends string & { supa?: () => number } = 'foo'">`
      );

      expect(
        source.slice(template.tag.pos.open.start, template.tag.pos.open.end)
      ).toBe(`<template>`);
    });

    it("multiple empty blocks", () => {
      const source = `
      <script></script>
      <template></template>
      <style></style>
    `;
      const result = doExtract(source);

      expect(result.map((x) => x.tag.type)).toEqual([
        "script",
        "template",
        "style",
      ]);
    });

    it("should catch multiple consecutive empty blocks", () => {
      const source = `<script></script><script setup></script><template></template>`;
      const result = doExtract(source); // catchEmptyBlocks

      expect(result.map((x) => x.tag.type)).toEqual([
        "script",
        "script",
        "template",
      ]);
    });
    it("should parse blocks back-to-back with no whitespace", () => {
      const source = `<script>foo</script><template><div>bar</div></template>`;
      const result = doExtract(source);
      // Expect 2 blocks, script & template
      expect(result).toHaveLength(2);
      expect(result.map((x) => x.tag.type)).toEqual(["script", "template"]);
    });

    it("should only extract 2 script blocks", () => {
      const source = `
        <script>console.log('foo')</script>
        <script setup>console.log('bar')</script>
        <script>console.log('baz')</script>
      `;
      const result = doExtract(source);
      expect(result).toHaveLength(2);
      expect(result.map((x) => x.tag.type)).toEqual(["script", "script"]);
    });
  });

  describe("findBlockLanguage", () => {
    function getFirstBlock(source: string) {
      const { descriptor } = parse(source);
      const blocks = extractBlocksFromDescriptor(descriptor);
      return blocks[0];
    }

    it("should return 'js' for a <script> block without lang", () => {
      const block = getFirstBlock(`<script>console.log('test')</script>`);
      const lang = findBlockLanguage(block);
      expect(lang).toBe("js");
    });

    it("should return the specific lang for a <script lang='ts'>", () => {
      const block = getFirstBlock(
        `<script lang="ts">console.log('test')</script>`
      );
      const lang = findBlockLanguage(block);
      expect(lang).toBe("ts");
    });

    it("should return 'html' for <template> without lang", () => {
      const block = getFirstBlock(`<template><div>Hello</div></template>`);
      const lang = findBlockLanguage(block);
      expect(lang).toBe("vue");
    });

    it("should return 'pug' if <template lang='pug'>", () => {
      const block = getFirstBlock(`<template lang="pug">div Hello</template>`);
      const lang = findBlockLanguage(block);
      expect(lang).toBe("pug");
    });

    it("should default to 'css' for <style> without lang", () => {
      const block = getFirstBlock(`<style>.foo { color: red; }</style>`);
      const lang = findBlockLanguage(block);
      expect(lang).toBe("css");
    });

    it("should return 'scss' for <style lang='scss'>", () => {
      const block = getFirstBlock(
        `<style lang="scss">$red: red; .foo {color: $red;}</style>`
      );
      const lang = findBlockLanguage(block);
      expect(lang).toBe("scss");
    });

    it("should return the tag name if lang is not set for a custom block", () => {
      const block = getFirstBlock(`<i18n> { "hello": "world" } </i18n>`);
      const lang = findBlockLanguage(block);
      expect(lang).toBe("i18n");
    });

    it("should return the custom lang if a custom block has lang='yaml'", () => {
      const block = getFirstBlock(`<docs lang="yaml">title: Hello</docs>`);
      const lang = findBlockLanguage(block);
      expect(lang).toBe("yaml");
    });

    it("should return 'gql' for a <graphql lang='gql'>", () => {
      const block = getFirstBlock(
        `<graphql lang="gql">query { hello }</graphql>`
      );
      const lang = findBlockLanguage(block);
      expect(lang).toBe("gql");
    });

    it("should fall back to the block type for an empty lang", () => {
      const block = getFirstBlock(
        `<script lang="">console.log('test')</script>`
      );
      const lang = findBlockLanguage(block);
      // Decide how your code handles this; maybe you want to fallback to "js" or an empty string
      expect(lang).toBe("js");
    });
  });

  describe("keepBlocks", () => {
    function resolveBlocks(source: string) {
      const { descriptor } = parse(source, {
        ignoreEmpty: false,
        templateParseOptions: { parseMode: "sfc" },
      });
      // Extract all blocks
      return extractBlocksFromDescriptor(descriptor);
    }

    it("should keep only the <script> block and remove others", () => {
      const source = `
        <script>console.log('script')</script>
        <template><div>template</div></template>
        <style>.foo { color: red; }</style>
        <custom-block>some content</custom-block>
      `;
      // Extract all blocks
      const blocks = resolveBlocks(source);
      // Prepare a MagicString for removal
      const s = new MagicString(source);

      // We only want to keep <script>
      const keptBlocks = keepBlocks(blocks, ["script"], s);
      expect(keptBlocks).toHaveLength(1);
      expect(keptBlocks[0].tag.type).toBe("script");

      // now let's see what the final output looks like
      const transformed = s.toString();

      // We expect the <template>, <style>, and <custom-block> to be removed
      expect(transformed).toContain("<script>console.log('script')</script>");
      expect(transformed).not.toContain("<template>");
      expect(transformed).not.toContain("<style>");
      expect(transformed).not.toContain("<custom-block>");
    });

    it("should keep multiple block types", () => {
      const source = `
        <script>console.log('script')</script>
        <script setup>console.log('setup')</script>
        <template><div>template</div></template>
        <style scoped>.foo { color: red; }</style>
      `;
      const blocks = resolveBlocks(source);
      const s = new MagicString(source);

      // Keep both scripts and the template
      const keptBlocks = keepBlocks(blocks, ["script", "template"], s);

      expect(keptBlocks).toHaveLength(3); // script + script setup + template
      expect(keptBlocks.map((b) => b.tag.type)).toEqual([
        "script",
        "script",
        "template",
      ]);

      const transformed = s.toString();
      // style should be removed
      expect(transformed).not.toContain("<style scoped>");
      // But scripts and template remain
      expect(transformed).toContain("<script>console.log('script')</script>");
      expect(transformed).toContain(
        "<script setup>console.log('setup')</script>"
      );
      expect(transformed).toContain("<template><div>template</div></template>");
    });

    it("should remove all blocks if keep list is empty", () => {
      const source = `
        <script></script>
        <template></template>
        <style></style>
      `;
      const blocks = resolveBlocks(source);
      const s = new MagicString(source);

      const keptBlocks = keepBlocks(blocks, [], s);
      expect(keptBlocks).toHaveLength(0);

      const transformed = s.toString();
      // Everything removed => only whitespace (possibly) remains
      expect(transformed).not.toContain("<script>");
      expect(transformed).not.toContain("<template>");
      expect(transformed).not.toContain("<style>");
    });

    it("should handle commented blocks between kept blocks", () => {
      const source = `
        <!-- <script>console.log('remove me')</script> -->
        <script>console.log('keep')</script>
        <!-- <template><div>remove me</div></template> -->
        <template><div>keep</div></template>
      `;
      const blocks = resolveBlocks(source);
      const s = new MagicString(source);

      // keep script + template
      const returnedBlocks = keepBlocks(blocks, ["script", "template"], s);
      const transformed = s.toString();

      expect(transformed).toContain("<script>console.log('keep')</script>");
      expect(transformed).toContain("<template><div>keep</div></template>");

      expect(returnedBlocks).toHaveLength(2);

      expect(transformed).toContain("remove me");
    });
  });

  describe("removeBlockTag", () => {
    function resolveBlocks(source: string) {
      const { descriptor } = parse(source, {
        ignoreEmpty: false,
        templateParseOptions: { parseMode: "sfc" },
      });
      // Extract all blocks
      return extractBlocksFromDescriptor(descriptor);
    }

    it("should remove only the script open/close tags, keeping the content", () => {
      const source = `<script>console.log('hello');</script>`;
      const blocks = resolveBlocks(source);

      // We'll remove the first (and only) block's tags
      const block = blocks[0];
      const s = new MagicString(source);

      removeBlockTag(block, s);
      const transformed = s.toString();

      // Expect open/close tags removed
      expect(transformed).toBe(`console.log('hello');`);
    });

    it("should remove only the template tags", () => {
      const source = `
        <template>
          <div>Hello</div>
        </template>
      `;
      const blocks = resolveBlocks(source);
      const block = blocks.find((b) => b.tag.type === "template");
      const s = new MagicString(source);

      removeBlockTag(block!, s);
      const transformed = s.toString().trim();

      // We want only the <div>Hello</div> lines, no <template> or </template>
      expect(transformed).toBe(
        `
  <div>Hello</div>
  `.trim()
      );
    });

    it("should handle a block with attributes in the opening tag", () => {
      const source = `<script lang="ts" setup>
        const x = 42;
      </script>`;
      const blocks = resolveBlocks(source);
      const block = blocks[0];
      const s = new MagicString(source);

      removeBlockTag(block, s);
      const transformed = s.toString().trim();

      // No <script lang="ts" setup> or </script> lines
      expect(transformed).toBe(`const x = 42;`);
    });

    it("should remove open/close tags but preserve internal comments", () => {
      const source = `<script>
        // some comment
        console.log('hello');
        // another comment
      </script>`;
      const blocks = resolveBlocks(source);
      const block = blocks[0];
      const s = new MagicString(source);

      removeBlockTag(block, s);
      const transformed = s.toString().trim();

      expect(transformed).toContain("// some comment");
      expect(transformed).toContain("console.log('hello');");
      expect(transformed).not.toContain("<script>");
      expect(transformed).not.toContain("</script>");
    });

    it("should match snapshot after removing block tags", () => {
      const content = "console.log('test')";
      const source = `<script lang="ts">${content}</script>`;
      const blocks = resolveBlocks(source);
      const s = new MagicString(source);

      removeBlockTag(blocks[0], s);
      expect(s.toString()).toBe(content);
    });
  });
});
