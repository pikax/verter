import { Range } from "vscode-languageserver-protocol";
import { VerterDocument } from "./verter.js";

// Create a subclass to access protected constructor
class TestVerterDocument extends VerterDocument {
  constructor(
    uri: string,
    languageId: string,
    version: number,
    content: string
  ) {
    super(uri, languageId, version, content);
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

    // it("should return empty string if the range start is greater than the end", () => {
    //   const invalidRange: Range = {
    //     start: { line: 1, character: 10 },
    //     end: { line: 1, character: 0 },
    //   };
    //   const text = doc.getText(invalidRange);
    //   expect(text).toBe("");
    // });

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
    it("should return the correct position for a given offset at the start", () => {
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

    it("should return the last valid position if offset is beyond the document length", () => {
      const totalLength = doc.getText().length; // length of entire content
      const posBeyond = doc.positionAt(totalLength + 1000);
      // Expecting it to clamp to the last character of the last line
      expect(posBeyond.line).toBe(2);
      expect(posBeyond.character).toBe(21); // '  It has three lines.' length is 22 zero-base
    });
  });

  describe("offsetAt()", () => {
    it("should return the correct offset for a given position at the start", () => {
      const offsetStart = doc.offsetAt({ line: 0, character: 0 });
      expect(offsetStart).toBe(0);
    });

    it("should return the correct offset for positions in subsequent lines", () => {
      // Line 0 length: "Hello world!" = 12 chars + 1 newline = 13
      // Position line 1, character 0 should be offset 13
      const offsetLine1Start = doc.offsetAt({ line: 1, character: 0 });
      expect(offsetLine1Start).toBe(13);

      // Now test a character in the last line
      // Line 1 length: "This is a test document." = 26 chars + 1 newline = 27 + previous 13 = 40 total by end of line 1
      // Actually counting precisely:
      // "Hello world!" → length = 12 + newline = 13
      // "This is a test document." → length = 26 + newline = 27
      // Total before line 2 starts = 13 + 27 = 40
      // On line 2, character 5 would be offset 40 + 5 = 45
      const offsetLine2Char5 = doc.offsetAt({ line: 2, character: 5 });
      expect(offsetLine2Char5).toBe(45);
    });

    it("should clamp the offset if the position is beyond the end of the document", () => {
      const offsetBeyond = doc.offsetAt({ line: 999, character: 999 });
      const totalLength = doc.getText().length;
      expect(offsetBeyond).toBe(totalLength);
    });

    it("should return 0 if the position is negative (invalid)", () => {
      const offsetNegative = doc.offsetAt({ line: -1, character: -1 });
      expect(offsetNegative).toBe(0);
    });
  });
});
