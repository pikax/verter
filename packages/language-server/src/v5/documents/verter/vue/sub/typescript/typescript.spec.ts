import { describe, it, expect, beforeEach } from "vitest";
import { VueTypescriptDocument, LanguageTypescript } from "./typescript.js";
import { SubDocumentProcessContext } from "../sub.js";
import { VueDocument } from "../../vue.js";
import { createSubDocumentUri } from "../../../../utils.js";

class MockTypeScriptElement extends VueTypescriptDocument {
  static create(
    uri: string,
    parent: VueDocument,
    languageId: LanguageTypescript,
    version: number
  ) {
    return new MockTypeScriptElement(uri, parent, languageId, version);
  }

  protected process(context: SubDocumentProcessContext): void {
    // nothing

    for (const block of context.blocks) {
      context.s.remove(
        block.block.tag.pos.open.start,
        block.block.tag.pos.open.end
      );
      context.s.remove(
        block.block.tag.pos.close.start,
        block.block.tag.pos.close.end
      );
    }
  }
}

describe("TypescriptDocument", () => {
  const parentUri = "file:///test.vue";
  const uri = createSubDocumentUri(parentUri, "options.tsx");
  const languageId: LanguageTypescript = "tsx";
  const version = 1;
  const initialContent = `const greet = (name: string) => {
  return "Hello, " + name;
}`;

  let doc: MockTypeScriptElement;
  let parentDocument: VueDocument;
  beforeEach(() => {
    parentDocument = VueDocument.create(
      parentUri,
      `<script>${initialContent}</script>`
    );

    doc = MockTypeScriptElement.create(uri, parentDocument, "tsx", version);
  });

  describe("constructor and basic properties", () => {
    it("should create a document with given properties and content", () => {
      expect(doc.uri).toBe(uri);
      expect(doc.languageId).toBe(languageId);
      expect(doc.version).toBe(version);
      expect(doc.getText()).toBe(initialContent);

      // The initial content has 2 lines
      const lineCount = initialContent.split("\n").length;
      expect(doc.lineCount).toBe(lineCount);
    });
  });

  describe("snapshot", () => {
    it("should create a snapshot on first access", () => {
      const snapshot = doc.snapshot;
      expect(snapshot).toBeDefined();
      expect(typeof snapshot.getText).toBe("function");

      const snapshotText = snapshot.getText(0, snapshot.getLength());
      expect(snapshotText).toBe(initialContent);
    });

    it("should return the same snapshot if document is not updated", () => {
      const snap1 = doc.snapshot;
      const snap2 = doc.snapshot;
      expect(snap1).toBe(snap2);
    });
  });

  describe("update()", () => {
    it("should update document content and increment version by default", () => {
      const newContent = "const x = 42;";
      const newVersion = version + 1;
      parentDocument.update(`<script>${newContent}</script>`, newVersion);
      expect(doc.getText()).toBe(newContent);
      expect(doc.version).toBe(newVersion);

      // After updating, snapshot should be recreated
      const newSnapshot = doc.snapshot;
      const snapText = newSnapshot.getText(0, newSnapshot.getLength());
      expect(snapText).toBe(newContent);
    });

    it("should update document content and set provided version", () => {
      const newContent = "const y = 123;";
      const newVersion = 10;
      parentDocument.update(`<script>${newContent}</script>`, newVersion);

      expect(doc.getText()).toBe(newContent);
      expect(doc.version).toBe(newVersion);

      const snapshot = doc.snapshot;
      const snapText = snapshot.getText(0, snapshot.getLength());
      expect(snapText).toBe(newContent);
    });

    it("should handle empty content updates", () => {
      parentDocument.update("");
      const text = doc.getText().trim();
      expect(text).toBe("");

      const snapshot = doc.snapshot;
      // empty lines
      expect(snapshot.getLength()).toBe(2);
    });

    it("should allow multiple updates and each time reset the snapshot", () => {
      parentDocument.update("<script>let a = 1;</script>");
      let snapshot = doc.snapshot;
      expect(snapshot.getText(0, snapshot.getLength())).toBe("let a = 1;");

      parentDocument.update("<script>let a = 1;\nlet b = 2;</script>");
      snapshot = doc.snapshot;
      expect(snapshot.getText(0, snapshot.getLength())).toBe(
        "let a = 1;\nlet b = 2;"
      );
      expect(doc.lineCount).toBe(2);
    });
  });
});
