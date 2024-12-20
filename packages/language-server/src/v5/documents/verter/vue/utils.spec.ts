import { describe, it, expect } from "vitest";
import { processBlocks, ProcessedBlock } from "./utils";
import { VerterASTBlock } from "@verter/core";
import { createSubDocument } from "../../utils.js";

describe("processBlocks", () => {
  function makeBlock(tag: string, lang?: string | true): VerterASTBlock {
    return {
      tag: { type: tag },
      block: { attrs: lang ? { lang } : {} },
    } as VerterASTBlock;
  }

  it("should return empty if there are no blocks", () => {
    const uri = "file:///some/path.vue";
    const result = processBlocks(uri, []);
    // No blocks mean no template/script/style, and no leftovers, so no custom block either
    expect(result).toHaveLength(0);
  });

  it("should handle a single template block without leftovers", () => {
    const uri = "file:///some/path.vue";
    const blocks = [makeBlock("template")];
    const result = processBlocks(uri, blocks);

    // Only a template block is processed, no leftovers means no custom block.
    expect(result).toHaveLength(1);

    const templateBlock = result.find((b) => b.type === "template")!;
    expect(templateBlock.languageId).toBe("tsx");
    expect(templateBlock.uri).toBe(createSubDocument(uri, "render.tsx"));
    expect(templateBlock.blocks).toHaveLength(1);
  });

  it("should handle multiple script blocks without leftovers", () => {
    const uri = "file:///some/path.vue";
    const scriptJs = makeBlock("script"); // no lang => js
    const scriptTs = makeBlock("script", "ts");
    const scriptTrue = makeBlock("script", true); // true => js

    const blocks = [scriptJs, scriptTs, scriptTrue];
    const result = processBlocks(uri, blocks);

    // Only script blocks processed, no leftovers => no custom block
    expect(result).toHaveLength(1);

    const scriptBlock = result.find((b) => b.type === "script")!;
    // First script block had no lang => languageId = 'js'
    expect(scriptBlock.languageId).toBe("js");
    expect(scriptBlock.uri).toBe(createSubDocument(uri, "options.tsx"));
    expect(scriptBlock.blocks).toHaveLength(3);
  });

  it("should handle style blocks with different lang attributes without leftovers", () => {
    const uri = "file:///some/path.vue";
    const styleNoLang = makeBlock("style"); // no lang => css
    const styleSass = makeBlock("style", "sass");
    const styleTrue = makeBlock("style", true); // true => css

    const blocks = [styleNoLang, styleSass, styleTrue];
    const result = processBlocks(uri, blocks);

    // Two style variants: css and sass, no leftovers => no custom block
    // We get exactly two processed style blocks: one for css and one for sass
    expect(result).toHaveLength(2);

    const cssBlock = result.find(
      (b) => b.type === "style" && b.languageId === "css"
    )!;
    const sassBlock = result.find(
      (b) => b.type === "style" && b.languageId === "sass"
    )!;

    expect(cssBlock.uri).toBe(createSubDocument(uri, "style.css"));
    expect(cssBlock.blocks).toHaveLength(2); // noLang and true are both css

    expect(sassBlock.uri).toBe(createSubDocument(uri, "style.sass"));
    expect(sassBlock.blocks).toHaveLength(1);
  });

  it("should put unknown tags into custom if no known blocks consume them", () => {
    const uri = "file:///some/path.vue";
    const blocks = [
      makeBlock("template"),
      makeBlock("unknown"),
      makeBlock("weird"),
    ];
    const result = processBlocks(uri, blocks);

    // Template is known, unknown and weird remain -> custom block produced
    expect(result).toHaveLength(2);

    const templateBlock = result.find((b) => b.type === "template")!;
    expect(templateBlock.blocks).toHaveLength(1);

    const customBlock = result.find((b) => b.type === "custom")!;
    expect(customBlock.blocks.map((b) => b.tag.type).sort()).toEqual([
      "unknown",
      "weird",
    ]);
    expect(customBlock.uri).toBe(createSubDocument(uri, "custom.temp"));
  });

  it("should handle multiple known and leftover blocks producing a custom block", () => {
    const uri = "file:///some/path.vue";
    const blocks = [
      makeBlock("template"),
      makeBlock("script", "ts"),
      makeBlock("style", "less"),
      makeBlock("customtag"),
      makeBlock("style"), // no lang => css
    ];
    const result = processBlocks(uri, blocks);

    // Known:
    // template: 1 block
    // script: 1 block (ts)
    // style: 2 blocks: one less, one css
    // leftover: customtag
    // total: template(1) + script(1) + style(2) + custom(1) = 5 blocks
    expect(result).toHaveLength(5);

    const templateBlock = result.find((b) => b.type === "template")!;
    expect(templateBlock.blocks).toHaveLength(1);

    const scriptBlock = result.find((b) => b.type === "script")!;
    expect(scriptBlock.languageId).toBe("ts");
    expect(scriptBlock.blocks).toHaveLength(1);

    const styleBlocks = result.filter((b) => b.type === "style");
    expect(styleBlocks).toHaveLength(2);
    const lessStyle = styleBlocks.find((b) => b.languageId === "less")!;
    const cssStyle = styleBlocks.find((b) => b.languageId === "css")!;
    expect(lessStyle.blocks).toHaveLength(1);
    expect(cssStyle.blocks).toHaveLength(1);

    const customBlock = result.find((b) => b.type === "custom")!;
    expect(customBlock.blocks).toHaveLength(1); // leftover customtag
    expect(customBlock.blocks[0].tag.type).toBe("customtag");
  });
});
