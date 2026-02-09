import { describe, it, expect, beforeEach, vi } from "vitest";
import { LanguageService, Stylesheet } from "vscode-css-languageservice";
import { ProcessedBlock } from "../../utils.js";
import { VueDocument } from "../../index.js";
import { VueStyleDocument } from "./VueStyleDocument";

describe("VueStyleDocument", () => {
  let mockLanguageService: LanguageService;
  let mockStylesheet: Stylesheet;
  let parentDoc: VueDocument;
  let styleDoc: VueStyleDocument;
  let styleUri: string;
  let styleBlock: ProcessedBlock;

  beforeEach(() => {
    // Mock a minimal LanguageService
    mockStylesheet = { mock: "styles" } as unknown as Stylesheet;
    mockLanguageService = {
      parseStylesheet: vi.fn().mockReturnValue(mockStylesheet),
    } as unknown as LanguageService;

    // Create a parent doc
    const parentContent = `
<template>
  <div>Hello</div>
</template>
<style>
.my-class { color: red; }
</style>
`;
    parentDoc = VueDocument.create("file:///parent.vue", parentContent, 1);

    // Get the actual style block from the parent's blocks
    const foundBlock = parentDoc.blocks.find((b) => b.type === "style");
    if (!foundBlock) {
      throw new Error("Style block not found in test setup");
    }
    styleBlock = foundBlock;
    styleUri = styleBlock.uri;
  });

  it("should instantiate correctly via static create()", () => {
    styleDoc = VueStyleDocument.create(
      styleUri,
      parentDoc,
      "css",
      mockLanguageService,
      1,
      styleBlock,
    );
    expect(styleDoc.uri).toBe(styleUri);
    expect(styleDoc.languageId).toBe("css");
    expect(styleDoc.version).toBe(1);
  });

  it("should parse stylesheet on first .stylesheet access", () => {
    styleDoc = VueStyleDocument.create(
      styleUri,
      parentDoc,
      "css",
      mockLanguageService,
      1,
      styleBlock,
    );
    // No parse yet
    expect(mockLanguageService.parseStylesheet).not.toHaveBeenCalled();

    // First access triggers parse
    const sheet = styleDoc.stylesheet;
    expect(sheet).toBe(mockStylesheet);
    expect(mockLanguageService.parseStylesheet).toHaveBeenCalledTimes(1);

    // Second access returns cached
    const sheet2 = styleDoc.stylesheet;
    expect(sheet2).toBe(sheet);
    expect(mockLanguageService.parseStylesheet).toHaveBeenCalledTimes(1);
  });

  it("should remove non-style blocks and strip <style> tags in process()", () => {
    styleDoc = VueStyleDocument.create(
      styleUri,
      parentDoc,
      "css",
      mockLanguageService,
      1,
      styleBlock,
    );

    // The first time we call getText(), it runs sync() => process()
    const text = styleDoc.getText();

    expect(text).toContain(".my-class { color: red; }");
    expect(text).not.toContain("<style>");
    expect(text).not.toContain("</style>");
    // The script block was removed
    expect(text).not.toContain("script");
    expect(text).not.toContain("<div>Hello</div>");
  });

  it("should re-parse stylesheet if doc updates (i.e., subDoc update)", () => {
    styleDoc = VueStyleDocument.create(
      styleUri,
      parentDoc,
      "css",
      mockLanguageService,
      1,
      styleBlock,
    );
    // First get => triggers parse
    styleDoc.getText();
    styleDoc.stylesheet; // parse once
    expect(mockLanguageService.parseStylesheet).toHaveBeenCalledTimes(1);

    // Now update the style doc (manually or via re-sync if parent's version changes)
    styleDoc.update(".my-class { color: blue; }", 2);
    // After an update, _stylesheet should be null => next access triggers new parse
    expect(mockLanguageService.parseStylesheet).toHaveBeenCalledTimes(1);

    const newSheet = styleDoc.stylesheet;
    expect(newSheet).toBe(mockStylesheet);
    // parseStylesheet called again
    expect(mockLanguageService.parseStylesheet).toHaveBeenCalledTimes(2);
  });

  it("should re-sync if parent version changes and blocks remain the same", () => {
    styleDoc = VueStyleDocument.create(
      styleUri,
      parentDoc,
      "css",
      mockLanguageService,
      1,
      styleBlock,
    );
    // Access => sync once
    styleDoc.getText();

    // Parent changes content (new version)
    parentDoc.update(parentDoc.getText().replace("red", "green"), 2);

    // Update the styleBlock reference to the new block from updated parent
    const newStyleBlock = parentDoc.blocks.find((b) => b.type === "style");
    if (newStyleBlock) {
      styleDoc.block = newStyleBlock;
    }

    // Next call => subDoc sees parent's version != subDoc.version, re-sync
    const updatedText = styleDoc.getText();
    expect(updatedText).toContain("green");
    expect(styleDoc.version).toBe(2);
  });

  it("should handle block with no matching parent blocks gracefully", () => {
    // Create a mock block that won't match the parent's blocks
    const badBlock: ProcessedBlock = {
      id: "bad-block",
      type: "style",
      uri: "file:///parent.vue._VERTER_.styleX.css",
      languageId: "css",
      blocks: [], // Empty blocks array means it won't find any matching blocks
    };
    // When block.blocks is empty, all parent blocks are removed and content is blanked
    const doc = VueStyleDocument.create(
      badBlock.uri,
      parentDoc,
      "css",
      mockLanguageService,
      1,
      badBlock,
    );
    const text = doc.getText();
    // The text should be processed but all blocks blanked out since none match
    expect(text).not.toContain("<template>");
    expect(text).not.toContain("<style>");
  });
});
