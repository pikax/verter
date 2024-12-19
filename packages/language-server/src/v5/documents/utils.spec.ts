import { describe, it, expect } from "vitest";
import { URI } from "vscode-uri";
import { TextDocument } from "vscode-languageserver-textdocument";
import { VueDocument } from "./verter/index.js";

import {
  isVueDocument,
  isVerterVirtual,
  isVueFile,
  pathToUri,
  pathToVerterVirtual,
  isVueSubDocument,
  uriToVerterVirtual,
  uriToPath,
} from "./utils.js";

class MockTextDocument implements TextDocument {
  uri: string;
  languageId: string;
  version: number;
  text: string;

  constructor(uri: string, languageId: string, text: string) {
    this.uri = uri;
    this.languageId = languageId;
    this.text = text;
    this.version = 1;
  }

  getText() {
    return this.text;
  }

  positionAt(offset: number) {
    return { line: 0, character: offset };
  }

  offsetAt(_position: { line: number; character: number }): number {
    return 0;
  }

  lineCount = 1;
}

class MockVueDocument extends VueDocument {
  constructor(uri: string, text: string) {
    // @ts-expect-error
    super(uri, text);
  }
}

describe("document utils", () => {
  describe("isVueDocument", () => {
    it("should return true if the document is an instance of VueDocument", () => {
      const doc = new MockVueDocument(
        "file:///some/path.vue",
        "<template></template>"
      );
      expect(isVueDocument(doc)).toBe(true);
    });

    it("should return false if the document is not a VueDocument", () => {
      const doc = new MockTextDocument(
        "file:///some/other.txt",
        "plaintext",
        "just text"
      );
      expect(isVueDocument(doc)).toBe(false);
    });

    it("should not return true if a non-VueDocument accidentally has languageId = 'vue'", () => {
      const doc = new MockTextDocument(
        "file:///some/fake.vue",
        "vue",
        "<template></template>"
      );
      // Our code first checks instance of VueDocument, which should fail here.
      expect(isVueDocument(doc)).toBe(false);
    });
  });

  describe("isVerterVirtual", () => {
    it("should return true if the URI starts with verter-virtual://", () => {
      const uri = "verter-virtual:///some/path.vue";
      expect(isVerterVirtual(uri)).toBe(true);
    });

    it("should return false if the URI does not start with verter-virtual://", () => {
      const uri = "file:///some/path.vue";
      expect(isVerterVirtual(uri)).toBe(false);
    });

    it("should not return true for an empty string", () => {
      const uri = "";
      expect(isVerterVirtual(uri)).toBe(false);
    });

    it("should not return true for a different scheme like http://", () => {
      const uri = "http://some/path.vue";
      expect(isVerterVirtual(uri)).toBe(false);
    });
  });

  describe("isVueFile", () => {
    it("should return true if the URI ends with .vue", () => {
      const uri = "file:///some/path.vue";
      expect(isVueFile(uri)).toBe(true);
    });

    it("should return false if the URI does not end with .vue", () => {
      const uri = "file:///some/path.js";
      expect(isVueFile(uri)).toBe(false);
    });

    it("should not return true if the extension is .VUE (uppercase)", () => {
      const uri = "file:///some/path.VUE";
      expect(isVueFile(uri)).toBe(false);
    });

    it("should not return true if the file ends with '.vue ' (trailing space)", () => {
      const uri = "file:///some/path.vue ";
      expect(isVueFile(uri)).toBe(false);
    });
  });

  describe("pathToUri", () => {
    it("should return the same string if it starts with file:///", () => {
      const filepath = "file:///some/path.vue";
      expect(pathToUri(filepath)).toBe(filepath);
    });

    it("should convert a filesystem path to a file:// URI", () => {
      const filepath = "/some/path.vue";
      const result = pathToUri(filepath);
      const parsed = URI.parse(result);
      expect(parsed.scheme).toBe("file");
      expect(parsed.fsPath.replace(/\\/g, "/")).toBe("/some/path.vue");
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
    it("should convert a filepath to a verter virtual URI if it's a vue file according to the logic (currently it just returns file://)", () => {
      const filepath = "/some/path.vue";
      const virtualUri = pathToVerterVirtual(filepath);
      const parsed = URI.parse(virtualUri);
      expect(parsed.scheme).toBe("file");
      expect(parsed.fsPath.replace(/\\/g, "/")).toBe("/some/path.vue");
    });

    it("should just return the same uri if it's not a vue file", () => {
      const filepath = "/some/path.js";
      const virtualUri = pathToVerterVirtual(filepath);
      const parsed = URI.parse(virtualUri);
      expect(parsed.scheme).toBe("file");
      expect(parsed.fsPath.replace(/\\/g, "/")).toBe("/some/path.js");
    });

    it("should not return a verter-virtual scheme if given a non-vue file", () => {
      const filepath = "/some/path.txt";
      const virtualUri = pathToVerterVirtual(filepath);
      expect(virtualUri).not.toMatch(/^verter-virtual:\/\//);
    });
  });

  describe("isVueSubDocument", () => {
    it("should return true if the URI matches the vue sub-document regex", () => {
      const uri = "file:///some/path.vue._VERTER_.template.js";
      expect(isVueSubDocument(uri)).toBe(true);
    });

    it("should return false if the URI does not match the vue sub-document regex", () => {
      const uri = "file:///some/path.vue";
      expect(isVueSubDocument(uri)).toBe(false);
    });

    it("should not return true if it partially matches but not fully", () => {
      // Missing the extension after ._VERTER_.part
      const uri = "file:///some/path.vue._VERTER_.template";
      expect(isVueSubDocument(uri)).toBe(false);
    });

    it("should not return true if the substring is reversed '.RETREV_'", () => {
      const uri = "file:///some/path.vue._RETREV_.template.js";
      expect(isVueSubDocument(uri)).toBe(false);
    });
  });

  describe("uriToVerterVirtual", () => {
    it("should return a file scheme URI if given a vue file that is not already verter-virtual", () => {
      const uri = "file:///some/path.vue";
      const newUri = uriToVerterVirtual(uri);
      const parsed = URI.parse(newUri);
      expect(parsed.scheme).toBe("file");
      expect(parsed.fsPath.replace(/\\/g, "/")).toBe("/some/path.vue");
    });

    it("should return the same URI if it is already verter-virtual", () => {
      const uri = "verter-virtual:///some/path.vue";
      expect(uriToVerterVirtual(uri)).toBe(uri);
    });

    it("should return file scheme if it's a vue sub-document", () => {
      const uri = "file:///some/path.vue._VERTER_.template.js";
      const converted = uriToVerterVirtual(uri);
      const parsed = URI.parse(converted);
      expect(parsed.scheme).toBe("file");
      expect(parsed.fsPath.replace(/\\/g, "/")).toBe(
        "/some/path.vue._VERTER_.template.js"
      );
    });

    it("should return the same uri if it's not a vue file or sub-document", () => {
      const uri = "file:///some/path.js";
      expect(uriToVerterVirtual(uri)).toBe(uri);
    });

    it("should not incorrectly prepend verter-virtual:// to a non-vue URI", () => {
      const uri = "http://example.com/path.vue.bak";
      const converted = uriToVerterVirtual(uri);
      // Since it's not a file:// scheme, uriToVerterVirtual will just return the same uri
      expect(converted).toBe(uri);
      expect(converted).not.toMatch(/^verter-virtual:\/\//);
    });
  });

  describe("uriToPath", () => {
    it("should return the fsPath for a file URI", () => {
      const uri = "file:///some/path.vue";
      const path = uriToPath(uri);
      expect(path).toBe("/some/path.vue");
    });

    it("should return the fsPath for a verter-virtual URI", () => {
      const uri = "verter-virtual:///some/path.vue";
      const path = uriToPath(uri);
      expect(path).toBe("/some/path.vue");
    });

    it("should return the original URI if the scheme is neither file nor verter-virtual", () => {
      const uri = "http:///some/path.vue";
      expect(uriToPath(uri)).toBe(uri);
    });

    it("should not modify a non-filesystem scheme beyond returning it as-is", () => {
      const uri = "data:text/plain;base64,SGVsbG8=";
      expect(uriToPath(uri)).toBe(uri);
    });

    it("should retrieve the vue document from sub", () => {
      const uri = "verter-virtual:///some/path.vue._VERTER_.bundle.js";
      expect(uriToPath(uri)).toBe("/some/path.vue");
    });
  });
});
