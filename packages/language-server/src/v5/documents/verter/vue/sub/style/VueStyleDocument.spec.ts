import { describe, it, expect, beforeEach, vi } from "vitest";
import { LanguageService, Stylesheet } from "vscode-css-languageservice";
import { VueStyleDocument } from "./VueStyleDocument";
import { VueDocument } from "../../index.js";

describe("VueStyleDocument", () => {
  let mockLanguageService: LanguageService;
  let mockStylesheet: Stylesheet;
  let parentDoc: VueDocument;
  let styleDoc: VueStyleDocument;
  let styleUri: string;

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

    // Indicate there's a style block in the parent's blocks array
    styleUri = "file:///parent.vue._VERTER_.style.css";
  });

  it("should instantiate correctly via static create()", () => {
    styleDoc = VueStyleDocument.create(
      styleUri,
      parentDoc,
      "css",
      mockLanguageService,
      1
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
      1
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
      1
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
      1
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
      1
    );
    // Access => sync once
    styleDoc.getText();

    // Parent changes content (new version)
    parentDoc.update(parentDoc.getText().replace("red", "green"), 2);

    // Next call => subDoc sees parent's version != subDoc.version, re-sync
    const updatedText = styleDoc.getText();
    expect(updatedText).toContain("green");
    expect(styleDoc.version).toBe(2);
  });

  it("should throw if block is not found in parent", () => {
    // Let's create a style doc with some random URI not in parent's blocks
    const badUri = "file:///parent.vue._VERTER_.styleX.css";
    expect(() => {
      VueStyleDocument.create(
        badUri,
        parentDoc,
        "css",
        mockLanguageService,
        1
      ).getText();
    }).toThrowError("Block not found!");
  });
});
