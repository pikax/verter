import { describe, it, expect, beforeEach, vi } from "vitest";
import { VueDocument } from "../vue.js";
import { VueSubDocument, SubDocumentProcessContext } from "./sub.js";
import { Position, Range } from "vscode-languageserver-textdocument";

// /**
//  * A minimal mock for VueDocument, just enough to make the sub-document logic work.
//  */
// class MockVueDocument extends VueDocument {
//   //   context: ParseContext;
//   //   blocks: Array<{ uri: string; blocks: VerterASTBlock[] }>;

//   static create(uri: string, content: string, version: number) {
//     // We skip calling super(...) because we presumably have a special constructor in real code.
//     // For test, just forcibly cast to VueDocument.
//     const doc = new MockVueDocument(uri, content, version);
//     return doc;
//   }

//   constructor(uri: string, content: string, version: number) {
//     // Minimal call to the real VueDocument or a similarly structured class
//     // @ts-expect-error - ignoring differences in constructor signatures
//     super(uri, content, version);

//     // Provide a minimal parse context with MagicString
//     const s = new MagicString(content);
//     this.context = {
//       s,
//       // possibly more parse context fields if needed
//     } as ParseContext;

//     // We track blocks that might reference subdocuments by `uri`
//     this.blocks = [];
//   }
// }

/**
 * A concrete subclass of VueSubDocument so we can test its behavior.
 */
class TestVueSubDocument extends VueSubDocument {
  constructor(
    uri: string,
    parent: VueDocument,
    languageId: string,
    version: number
  ) {
    super(uri, parent, languageId, version);
  }

  /**
   * Minimal implementation of process(). For example, let's say we remove all "foo" from the text.
   */
  protected process(context: SubDocumentProcessContext): void {
    // E.g., remove "foo" from the text as a demonstration
    const index = context.s.toString().indexOf("foo");
    if (index !== -1) {
      context.s.remove(index, index + 3); // remove the substring "foo"
    }

    context.s.replace(`<script lang="ts">`, "");
    context.s.replace(`<script>`, "");
    context.s.replace("</script>", "");
  }
}

describe("VueSubDocument", () => {
  let parentDoc: VueDocument;
  let subUri: string;
  let subDoc: TestVueSubDocument;
  let blockContent: string;

  beforeEach(() => {
    blockContent = "const foo = 42;";
    parentDoc = VueDocument.create(
      "file:///parent.vue",
      `<script lang="ts">${blockContent}</script>`,
      1
    );

    // Suppose we have one block referencing subDoc
    subUri = "file:///parent.vue._VERTER_.options.tsx";
  });

  it("should throw if block not found in parent", () => {
    const badUri = "file:///parent.vue._VERTER_.template.ts";
    // No block with this URI
    expect(() => {
      new TestVueSubDocument(badUri, parentDoc, "typescript", 1).getText();
    }).toThrowError("Block not found!");
  });

  it("should sync content from parent and process it on first access", () => {
    subDoc = new TestVueSubDocument(subUri, parentDoc, "typescript", 1);

    // The first time we call getText, it should sync and process the content.
    const text = subDoc.getText();
    // Our process method removes "foo", so the original "const foo = 42;" becomes "const  = 42;"
    expect(text).toBe("const  = 42;");
    // Also, version should match parent's version
    expect(subDoc.version).toBe(parentDoc.version);
  });

  it("should re-sync if parent's version changes", () => {
    subDoc = new TestVueSubDocument(subUri, parentDoc, "typescript", 1);

    // // first access, triggers sync
    // subDoc.getText();

    // now let's update the parent
    parentDoc.update("<script>let bar = 99;</script>", 2);
    // subDoc's version is still 1, parent's version is now 2
    // next access should trigger re-sync
    const text = subDoc.getText();
    expect(text).toBe("let bar = 99;");
    expect(subDoc.version).toBe(2); // subDoc updated to match parent version
  });

  it("should update local content and sourceMap on each sync", () => {
    subDoc = new TestVueSubDocument(subUri, parentDoc, "typescript", 1);

    // first get => triggers sync, removes "foo"
    let text = subDoc.getText();
    expect(text).toBe("const  = 42;");

    // let's add "foo" again in parent
    parentDoc.update("<script>'foo foo foo'</script>", 2);
    // next access => re-sync => process removes "foo"
    text = subDoc.getText();
    // all "foo" removed => just spaces remain
    expect(text.trim()).toBe("' foo foo'");
    expect(subDoc.version).toBe(2);
  });

  describe("source map transformations", () => {
    beforeEach(() => {
      // We set up subDoc
      subDoc = new TestVueSubDocument(subUri, parentDoc, "typescript", 1);
    });

    it("toGeneratedPosition / toGeneratedRange should map original to subDoc generated", () => {
      const contentIndex = parentDoc.getText().indexOf("const");
      const pos: Position = { line: 0, character: contentIndex };
      const genPos = subDoc.toGeneratedPosition(pos);
      expect(genPos.line).toBe(0);

      const range: Range = {
        start: { line: 0, character: contentIndex },
        end: {
          line: 0,
          character: contentIndex + blockContent.length + 120000,
        },
      };

      const genRange = subDoc.toGeneratedRange(range);
      expect(genRange.start.line).toBeGreaterThanOrEqual(0);
      expect(genRange.end.character).toBeGreaterThanOrEqual(0);

      expect(subDoc.getText(genRange)).toBe("const  = 42;");
    });

    it("toOriginalPosition / toOriginalRange should map from subDoc to parent's original", () => {
      // Let's pick an offset in the subDoc
      const offset = subDoc.getText().indexOf("42");
      const posInSub = subDoc.positionAt(offset);
      const origPos = subDoc.toOriginalPosition(posInSub);
      // Check that line is not negative, indicating a valid mapping
      expect(origPos.line).toBeGreaterThanOrEqual(0);

      // Similarly for a range
      const range: Range = {
        start: { line: 0, character: 0 },
        end: posInSub,
      };
      const origRange = subDoc.toOriginalRange(range);
      expect(origRange.start.line).toBeGreaterThanOrEqual(0);
      expect(origRange.end.character).toBeGreaterThanOrEqual(0);
      expect(parentDoc.getText(origRange)).toBe("const foo = ");
    });

    it("toGeneratedOffsetFromPosition should convert an original position to subDoc offset", () => {
      const originalPos: Position = {
        line: 0,
        character: parentDoc.getText().indexOf("foo"),
      };
      const genOffset = subDoc.toGeneratedOffsetFromPosition(originalPos);
      expect(genOffset).toBe(5);
    });

    it("toOriginalOffset should convert subDoc offset to original offset in parent", () => {
      const subOffset = subDoc.getText().indexOf("42");
      const origOffset = subDoc.toOriginalOffset(subOffset);
      expect(origOffset).toBeGreaterThanOrEqual(30);
    });
  });
});
