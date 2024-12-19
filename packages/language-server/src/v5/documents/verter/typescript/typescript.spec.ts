import { describe, it, expect, beforeEach } from "vitest";
import { TypescriptDocument, LanguageTypescript } from "./typescript.js";
import { VerterDocument } from "../verter.js";

describe("TypescriptDocument", () => {
  const uri = "file:///test.ts";
  const languageId: LanguageTypescript = "ts";
  const version = 1;
  const initialContent = `const greet = (name: string) => {
  return "Hello, " + name;
}`;

  let doc: TypescriptDocument;
  beforeEach(() => {
    doc = TypescriptDocument.create(uri, languageId, version, initialContent);
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
      doc.update(newContent);
      expect(doc.getText()).toBe(newContent);
      expect(doc.version).toBe(version + 1);

      // After updating, snapshot should be recreated
      const newSnapshot = doc.snapshot;
      const snapText = newSnapshot.getText(0, newSnapshot.getLength());
      expect(snapText).toBe(newContent);
    });

    it("should update document content and set provided version", () => {
      const newContent = "const y = 123;";
      const newVersion = 10;
      doc.update(newContent, newVersion);

      expect(doc.getText()).toBe(newContent);
      expect(doc.version).toBe(newVersion);

      const snapshot = doc.snapshot;
      const snapText = snapshot.getText(0, snapshot.getLength());
      expect(snapText).toBe(newContent);
    });

    it("should handle empty content updates", () => {
      doc.update("");
      expect(doc.getText()).toBe("");
      // Empty content should still mean at least one line in the document
      expect(doc.lineCount).toBe(1);

      const snapshot = doc.snapshot;
      expect(snapshot.getLength()).toBe(0);
    });

    it("should allow multiple updates and each time reset the snapshot", () => {
      doc.update("let a = 1;");
      let snapshot = doc.snapshot;
      expect(snapshot.getText(0, snapshot.getLength())).toBe("let a = 1;");

      doc.update("let a = 1;\nlet b = 2;");
      snapshot = doc.snapshot;
      expect(snapshot.getText(0, snapshot.getLength())).toBe(
        "let a = 1;\nlet b = 2;"
      );
      expect(doc.lineCount).toBe(2);
    });
  });
});
