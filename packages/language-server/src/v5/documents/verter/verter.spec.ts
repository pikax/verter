import { describe, it, expect, beforeEach } from "vitest";
import { TextEdit, Range, Position } from "vscode-languageserver-protocol";
import { VerterDocument } from "./verter.js";

// Create a subclass to access protected constructor
class TestVerterDocument extends VerterDocument {
  getDocFn: Function = vi.fn();

  constructor(
    uri: string,
    languageId: string,
    version: number,
    content: string
  ) {
    super(uri, languageId, version, content);
  }

  get doc() {
    this.getDocFn();
    return super.doc;
  }
}

describe("VerterDocument", () => {
  const uri = "file://test.txt";
  const languageId = "plaintext";
  const version = 1;
  const content = `Hello world!
This is a test document.
It has three lines.`;

  let doc: TestVerterDocument;
  beforeEach(() => {
    doc = new TestVerterDocument(uri, languageId, version, content);
  });

  describe("constructor and basic properties", () => {
    it("should create a document with the given properties", () => {
      expect(doc.uri).toBe(uri);
      expect(doc.languageId).toBe(languageId);
      expect(doc.version).toBe(version);
      // There are 3 lines in the content
      expect(doc.lineCount).toBe(3);
    });

    describe("doc accessors", () => {
      it("should update the doc in the constructor", () => {
        expect(doc.getDocFn).not.toHaveBeenCalled();
      });
      it("it should call on update", () => {
        doc.update("test");
        expect(doc.getDocFn).toHaveBeenCalledOnce();
      });
    });
  });

  describe("getText()", () => {
    it("should return the entire document text if no range is provided", () => {
      const text = doc.getText();
      expect(text).toBe(content);
    });

    it("should return text within a given valid range", () => {
      const range: Range = {
        start: { line: 0, character: 6 }, // Start at 'w' of 'Hello world!'
        end: { line: 0, character: 11 }, // End at 'd' of 'world'
      };
      const text = doc.getText(range);
      expect(text).toBe("world");
    });

    it("should return empty string if the range is completely out of bounds", () => {
      const outOfBoundsRange: Range = {
        start: { line: 100, character: 0 },
        end: { line: 101, character: 10 },
      };
      const text = doc.getText(outOfBoundsRange);
      expect(text).toBe("");
    });
  });

  describe("positionAt()", () => {
    it("should return the correct position for offset=0 (start of document)", () => {
      const pos = doc.positionAt(0);
      expect(pos.line).toBe(0);
      expect(pos.character).toBe(0);
    });

    it("should return the correct position in the middle of the document", () => {
      // Find offset at 'w' in "Hello world!"
      const offset = content.indexOf("world");
      const posWorld = doc.positionAt(offset);
      expect(posWorld.line).toBe(0);
      expect(posWorld.character).toBe(6);
    });

    it("should clamp position to the last character if offset > length", () => {
      const totalLength = doc.getText().length;
      const posBeyond = doc.positionAt(totalLength + 1000);
      // Expecting it to clamp to the last character of the last line
      expect(posBeyond.line).toBe(2);
      // "It has three lines." length is 22 chars, indexing from 0 → last char at pos 21
      expect(posBeyond.character).toBe(19);
    });
  });

  describe("offsetAt()", () => {
    it("should return the correct offset for the start position", () => {
      const offsetStart = doc.offsetAt({ line: 0, character: 0 });
      expect(offsetStart).toBe(0);
    });

    it("should return the correct offset for positions in subsequent lines", () => {
      const offsetLine1Start = doc.offsetAt({ line: 1, character: 0 });
      expect(offsetLine1Start).toBe(13);

      const offsetLine2Char5 = doc.offsetAt({ line: 2, character: 5 });
      expect(offsetLine2Char5).toBe(43);
    });

    it("should clamp offset if position is beyond document end", () => {
      const offsetBeyond = doc.offsetAt({ line: 999, character: 999 });
      const totalLength = doc.getText().length;
      expect(offsetBeyond).toBe(totalLength);
    });

    it("should return 0 if line or character is negative", () => {
      const offsetNegative = doc.offsetAt({ line: -1, character: -1 });
      expect(offsetNegative).toBe(0);
    });
  });

  describe("update()", () => {
    it("should update the document content and increment version by default", () => {
      const newContent = "New content";
      doc.update(newContent);
      expect(doc.getText()).toBe(newContent);
      expect(doc.version).toBe(version + 1);
      expect(doc.lineCount).toBe(1);
    });

    it("should update the document content and use provided version", () => {
      const newContent = "Another content";
      const newVersion = 10;
      doc.update(newContent, newVersion);
      expect(doc.getText()).toBe(newContent);
      expect(doc.version).toBe(newVersion);
      expect(doc.lineCount).toBe(1);
    });

    it("should handle overriding with empty content", () => {
      doc.update("");
      expect(doc.getText()).toBe("");
      expect(doc.lineCount).toBe(1); // an empty doc is considered 1 line
    });

    it("should allow positionAt and offsetAt calls after update", () => {
      const newContent = "Short text";
      doc.update(newContent);

      // Check offsets and positions on the new content
      const pos = doc.positionAt(newContent.indexOf("text"));
      expect(pos.line).toBe(0);
      expect(pos.character).toBe(6); // "Short " = 6 chars, 't' at idx 6

      const offset = doc.offsetAt({ line: 0, character: 3 });
      expect(newContent[offset]).toBe("r"); // 'r' is at index 3 in "Short"
    });
  });

  describe("applyEdits()", () => {
    it("should apply a single edit and return updated text", () => {
      const edits: TextEdit[] = [
        {
          range: {
            start: { line: 0, character: 6 }, // Replace "world" with "universe"
            end: { line: 0, character: 11 },
          },
          newText: "universe",
        },
      ];
      const updated = doc.applyEdits(edits);
      expect(updated).toContain("Hello universe!");
      expect(updated).not.toContain("Hello world!");
      // Original doc is not changed
      expect(doc.getText()).toBe(content);
    });

    it("should apply multiple edits and return updated text", () => {
      const edits: TextEdit[] = [
        {
          range: {
            start: { line: 0, character: 6 }, // Replace "world" with "universe"
            end: { line: 0, character: 11 },
          },
          newText: "universe",
        },
        {
          range: {
            start: { line: 1, character: 5 }, // Insert 'amended '
            end: { line: 1, character: 5 },
          },
          newText: "amended ",
        },
      ];
      const updated = doc.applyEdits(edits);
      expect(updated).toContain("Hello universe!");
      expect(updated).toContain("This amended is a test document.");
      // Again, original doc unchanged
      expect(doc.getText()).toBe(content);
    });

    it("should handle insertion edit at the start of the document", () => {
      const edits: TextEdit[] = [
        {
          range: {
            start: { line: 0, character: 0 },
            end: { line: 0, character: 0 },
          },
          newText: "Welcome! ",
        },
      ];
      const updated = doc.applyEdits(edits);
      expect(updated.startsWith("Welcome! Hello world!")).toBe(true);
    });

    it("should handle deletion edit at the end of the document", () => {
      const lastPos = doc.positionAt(Number.MAX_SAFE_INTEGER);
      const edits: TextEdit[] = [
        {
          range: {
            start: { line: lastPos.line, character: lastPos.character - 7 },
            end: { line: lastPos.line, character: lastPos.character },
          },
          newText: "",
        },
      ];
      const updated = doc.applyEdits(edits);
      expect(updated.endsWith("It has three")).toBe(true);
    });

    it("should append out-of-range edits", () => {
      const edits: TextEdit[] = [
        {
          range: {
            start: { line: 100, character: 0 },
            end: { line: 101, character: 10 },
          },
          newText: "END",
        },
      ];
      const updated = doc.applyEdits(edits);
      expect(updated).toBe(doc.getText() + "END");
    });
  });
});
