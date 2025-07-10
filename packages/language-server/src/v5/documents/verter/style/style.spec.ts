import { describe, it, expect, beforeEach, vi } from "vitest";
import { LanguageService, Stylesheet } from "vscode-css-languageservice";
import { StyleDocument, LanguageStyle } from "./style";

// Mock the LanguageService
const mockLanguageService: LanguageService = {
  parseStylesheet: vi.fn().mockImplementation(
    () =>
      ({
        mock: "stylesheet",
      } as unknown as Stylesheet)
  ),
  // Add any other required LanguageService methods if needed
} as unknown as LanguageService;

describe("StyleDocument", () => {
  const uri = "file:///styles.css";
  const languageId: LanguageStyle = "css";
  const version = 1;
  const initialContent = `body {
  color: red;
}`;

  let doc: StyleDocument;
  beforeEach(() => {
    vi.clearAllMocks();
    doc = StyleDocument.create(
      mockLanguageService,
      uri,
      languageId,
      version,
      initialContent
    );
  });

  describe("constructor and basic properties", () => {
    it("should create a document with given properties and content", () => {
      expect(doc.uri).toBe(uri);
      expect(doc.languageId).toBe(languageId);
      expect(doc.version).toBe(version);
      expect(doc.getText()).toBe(initialContent);

      // Count lines in initialContent
      const lineCount = initialContent.split("\n").length;
      expect(doc.lineCount).toBe(lineCount);
    });
  });

  describe("stylesheet", () => {
    it("should call parseStylesheet once and cache the result", () => {
      const sheet1 = doc.stylesheet;
      expect(sheet1).toEqual({ mock: "stylesheet" });
      // parseStylesheet should have been called once
      expect(mockLanguageService.parseStylesheet).toHaveBeenCalledTimes(1);

      const sheet2 = doc.stylesheet;
      expect(sheet2).toBe(sheet1);
      // Still only one call, since it is cached
      expect(mockLanguageService.parseStylesheet).toHaveBeenCalledTimes(1);
    });

    it("should only call parseStylesheet when accessing doc.stylesheet", () => {
      expect(mockLanguageService.parseStylesheet).not.toHaveBeenCalled();
      doc.stylesheet;
      expect(mockLanguageService.parseStylesheet).toHaveBeenCalledTimes(1);
    });
  });

  describe("update()", () => {
    it("should update the document content and increment version by default", () => {
      const newContent = `p { font-size: 14px; }`;
      doc.update(newContent);

      expect(doc.getText()).toBe(newContent);
      expect(doc.version).toBe(version + 1);

      // After updating, parseStylesheet should be called again upon next stylesheet access
      const newSheet = doc.stylesheet;
      expect(newSheet).toEqual({ mock: "stylesheet" });
      expect(mockLanguageService.parseStylesheet).toHaveBeenCalledTimes(1); // Once for old content + once now after reset
    });

    it("should update document content and set provided version", () => {
      const newContent = `a { text-decoration: none; }`;
      const newVersion = 10;
      doc.update(newContent, newVersion);

      expect(doc.getText()).toBe(newContent);
      expect(doc.version).toBe(newVersion);
    });

    it("should reset the stylesheet after update so that a new one is created", () => {
      const oldSheet = doc.stylesheet;
      doc.update("div { margin: 0; }");

      const newSheet = doc.stylesheet;
      expect(newSheet).not.toBe(oldSheet);
      // parseStylesheet called twice: once before update, once after update
      expect(mockLanguageService.parseStylesheet).toHaveBeenCalledTimes(2);
    });

    it("should handle empty content updates", () => {
      doc.stylesheet;
      doc.update("");
      expect(doc.getText()).toBe("");
      expect(doc.lineCount).toBe(1); // Even empty doc considered one line

      const snap = doc.stylesheet;
      expect(snap).toEqual({ mock: "stylesheet" });
      // parseStylesheet called twice total: once initially, once now
      expect(mockLanguageService.parseStylesheet).toHaveBeenCalledTimes(2);
    });

    it("should allow multiple updates and each time reset the stylesheet", () => {
      doc.update("h1 { color: blue; }");
      doc.stylesheet;
      doc.update("h1 { color: green; }");
      doc.stylesheet;

      // parseStylesheet called twice total:
      // 1 after first update
      // 1 after second update
      expect(mockLanguageService.parseStylesheet).toHaveBeenCalledTimes(2);
    });
  });
});
