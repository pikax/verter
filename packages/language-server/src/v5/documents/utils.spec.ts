import { describe, it, expect } from "vitest";
import { URI } from "vscode-uri";
import { TextDocument } from "vscode-languageserver-textdocument";
import { VerterDocument, VueDocument } from "./verter/index.js";

import {
  isVueDocument,
  isVerterVirtual,
  isVueFile,
  pathToUri,
  pathToVerterVirtual,
  isVueSubDocument,
  uriToVerterVirtual,
  uriToPath,
  createSubDocument,
  VerterVirtualFileScheme,
} from "./utils.js";

// Create a subclass to access protected constructor
class TestVerterDocument extends VerterDocument {
  static create(
    uri: string,
    languageId: string,
    content: string,
    version?: number
  ) {
    return new VerterDocument(uri, languageId, version ?? -1, content);
  }
  constructor(
    uri: string,
    languageId: string,
    version: number,
    content: string
  ) {
    super(uri, languageId, version, content);
  }
}

describe("document utils", () => {
  describe("isVueDocument", () => {
    it("should return true if the document is an instance of VueDocument", () => {
      const doc = VueDocument.create(
        "file:///some/path.vue",
        "<template></template>"
      );
      expect(isVueDocument(doc)).toBe(true);
    });

    it("should return false if the document is not a VueDocument", () => {
      const doc = TestVerterDocument.create(
        "file:///some/other.txt",
        "plaintext",
        "just text"
      );
      expect(isVueDocument(doc)).toBe(false);
    });

    it("should not return true if a non-VueDocument has languageId='vue'", () => {
      const doc = TestVerterDocument.create(
        "file:///some/fake.vue",
        "vue",
        "<template></template>"
      );
      expect(isVueDocument(doc)).toBe(false);
    });
  });

  describe("isVerterVirtual", () => {
    it("returns true if the URI starts with verter-virtual://", () => {
      const uri = `${VerterVirtualFileScheme}:///some/path.vue`;
      expect(isVerterVirtual(uri)).toBe(true);
    });

    it("returns false for a file URI", () => {
      const uri = "file:///some/path.vue";
      expect(isVerterVirtual(uri)).toBe(false);
    });

    it("returns false for an empty string", () => {
      const uri = "";
      expect(isVerterVirtual(uri)).toBe(false);
    });

    it("returns false for http scheme", () => {
      const uri = "http://some/path.vue";
      expect(isVerterVirtual(uri)).toBe(false);
    });
  });

  describe("isVueFile", () => {
    it("returns true if the URI ends with .vue", () => {
      const uri = "file:///some/path.vue";
      expect(isVueFile(uri)).toBe(true);
    });

    it("returns false if no .vue extension", () => {
      const uri = "file:///some/path.js";
      expect(isVueFile(uri)).toBe(false);
    });

    it("returns false for uppercase .VUE", () => {
      const uri = "file:///some/path.VUE";
      expect(isVueFile(uri)).toBe(false);
    });

    it("returns false if trailing space after .vue", () => {
      const uri = "file:///some/path.vue ";
      expect(isVueFile(uri)).toBe(false);
    });
  });

  describe("pathToUri", () => {
    it("returns the same string if it starts with file:///", () => {
      const filepath = "file:///some/path.vue";
      expect(pathToUri(filepath)).toBe(filepath);
    });

    it("converts a filesystem path to file:// URI", () => {
      const filepath = "/some/path.vue";
      const result = pathToUri(filepath);
      const parsed = URI.parse(result);
      expect(parsed.scheme).toBe("file");
      expect(parsed.fsPath.replace(/\\/g, "/")).toBe("/some/path.vue");
    });

    it("handles a relative path by converting to a file:// URI", () => {
      const filepath = "relative/path.vue";
      const result = pathToUri(filepath);
      const parsed = URI.parse(result);
      expect(parsed.scheme).toBe("file");
      // The exact fsPath may vary based on OS, we just ensure it's file scheme now.
      expect(parsed.path).toContain("relative");
    });

    it("should not modify non-file paths such as 'http://example.com'", () => {
      const filepath = "http://example.com";
      // It will still try to convert this to a file URI, so let's assert that it doesn't
      // return the same string.
      const result = pathToUri(filepath);
      expect(result).not.toBe(filepath);
      // Since pathToUri uses URI.file(), it can't handle non-filesystem paths directly
      // So let's just check scheme:
      const parsed = URI.parse(result);
      expect(parsed.scheme).toBe("file");
    });
  });

  describe("pathToVerterVirtual", () => {
    it("returns a file:// uri if vue file", () => {
      const filepath = "/some/path.vue";
      const virtualUri = pathToVerterVirtual(filepath);
      const parsed = URI.parse(virtualUri);
      expect(parsed.scheme).toBe("file");
      expect(parsed.fsPath.replace(/\\/g, "/")).toBe("/some/path.vue");
    });

    it("returns same uri as file if not a vue file", () => {
      const filepath = "/some/path.js";
      const virtualUri = pathToVerterVirtual(filepath);
      const parsed = URI.parse(virtualUri);
      expect(parsed.scheme).toBe("file");
      expect(parsed.fsPath.replace(/\\/g, "/")).toBe("/some/path.js");
    });

    it("does not convert to verter-virtual if given a non-vue file", () => {
      const filepath = "/some/path.txt";
      const virtualUri = pathToVerterVirtual(filepath);
      expect(virtualUri).not.toMatch(/^verter-virtual:\/\//);
    });
  });

  describe("isVueSubDocument", () => {
    it("returns true if matches vue sub-document pattern", () => {
      const uri = "file:///some/path.vue._VERTER_.template.js";
      expect(isVueSubDocument(uri)).toBe(true);
    });

    it("returns false if no match", () => {
      const uri = "file:///some/path.vue";
      expect(isVueSubDocument(uri)).toBe(false);
    });

    it("returns false if incomplete pattern", () => {
      const uri = "file:///some/path.vue._VERTER_.template";
      expect(isVueSubDocument(uri)).toBe(false);
    });

    it("returns false if reversed substring", () => {
      const uri = "file:///some/path.vue._RETREV_.template.js";
      expect(isVueSubDocument(uri)).toBe(false);
    });
  });

  describe("uriToVerterVirtual", () => {
    it("returns file scheme if given a vue file that is not already verter-virtual", () => {
      const uri = "file:///some/path.vue";
      const newUri = uriToVerterVirtual(uri);
      const parsed = URI.parse(newUri);
      expect(parsed.scheme).toBe("file");
      expect(parsed.fsPath.replace(/\\/g, "/")).toBe("/some/path.vue");
    });

    it("returns same URI if it's already verter-virtual", () => {
      const uri = "verter-virtual:///some/path.vue";
      expect(uriToVerterVirtual(uri)).toBe(uri);
    });

    it("returns file scheme if it's a vue sub-document", () => {
      const uri = "file:///some/path.vue._VERTER_.template.js";
      const converted = uriToVerterVirtual(uri);
      const parsed = URI.parse(converted);
      expect(parsed.scheme).toBe("file");
      expect(parsed.fsPath.replace(/\\/g, "/")).toBe(
        "/some/path.vue._VERTER_.template.js"
      );
    });

    it("returns same uri if not vue or sub-document", () => {
      const uri = "file:///some/path.js";
      expect(uriToVerterVirtual(uri)).toBe(uri);
    });

    it("does not prepend verter-virtual for non-file schemes", () => {
      const uri = "http://example.com/path.vue.bak";
      const converted = uriToVerterVirtual(uri);
      expect(converted).toBe(uri);
      expect(converted).not.toMatch(/^verter-virtual:\/\//);
    });

    it("handles unexpected schemes gracefully", () => {
      const uri = "unknown-scheme:///some/path.vue";
      // It's a vue file but with an unknown scheme. According to code,
      // it tries to convert to file scheme if isVueFile returns true.
      const converted = uriToVerterVirtual(uri);
      const parsed = URI.parse(converted);
      expect(parsed.scheme).toBe("file");
    });
  });

  describe("uriToPath", () => {
    it("returns fsPath for file URI", () => {
      const uri = "file:///some/path.vue";
      const path = uriToPath(uri);
      expect(path).toBe("/some/path.vue");
    });

    it("returns fsPath for verter-virtual URI", () => {
      const uri = "verter-virtual:///some/path.vue";
      const path = uriToPath(uri);
      expect(path).toBe("/some/path.vue");
    });

    it("returns original URI if unknown scheme", () => {
      const uri = "http:///some/path.vue";
      expect(uriToPath(uri)).toBe(uri);
    });

    it("returns same URI if data scheme (non-file)", () => {
      const uri = "data:text/plain;base64,SGVsbG8=";
      expect(uriToPath(uri)).toBe(uri);
    });

    it("retrieves the vue document path from sub-document", () => {
      const uri = "verter-virtual:///some/path.vue._VERTER_.bundle.js";
      expect(uriToPath(uri)).toBe("/some/path.vue");
    });

    it("normalizes Windows backslashes", () => {
      const uri = "file:///C:/Users/Name/project/file.vue";
      const path = uriToPath(uri);
      expect(path).toBe("c:/Users/Name/project/file.vue");
    });
  });

  describe("createSubDocument", () => {
    it("creates a sub-document URI with the given ending", () => {
      const uri = "file:///some/path.vue";
      const ending = "template.js";
      const subUri = createSubDocument(uri, ending);
      expect(subUri).toBe("file:///some/path.vue._VERTER_.template.js");
    });

    it("appends the required prefix before the ending", () => {
      const uri = "file:///some/path.vue";
      const ending = "script.ts";
      const subUri = createSubDocument(uri, ending);
      expect(subUri).toBe("file:///some/path.vue._VERTER_.script.ts");
    });

    it("throw with non-vue URIs", () => {
      const uri = "file:///some/path.txt";
      const ending = "data.json";
      expect(() => createSubDocument(uri, ending)).toThrowError();
    });

    it("createSubDocument should be compatible with isVueSubDocument", () => {
      expect(
        isVueSubDocument(
          createSubDocument("file:///some/path.vue", "bundle.ts")
        )
      );
    });
  });
});
