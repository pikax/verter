import { NodeTypes, type CommentNode } from "@vue/compiler-core";
import { handleComment } from "./index.js";

describe("parser template comment", () => {
  it("should return comment", () => {
    const node = {
      content: " comment ",
      type: NodeTypes.COMMENT,
      loc: {
        source: "<!-- comment -->",
      },
    } as CommentNode;

    expect(handleComment(node)).toMatchObject({
      content: " comment ",
      node,
    });
  });

  it('should return null if node type is not "COMMENT"', () => {
    const node = {
      content: " comment ",
      type: NodeTypes.ELEMENT,
      loc: {
        source: "<!-- comment -->",
      },
    } as any as CommentNode;

    expect(handleComment(node)).toBeNull();
  });

  it("should support multi-line", () => {
    const content = Array.from({ length: 20 })
      .map((_, i) => "comment " + i)
      .join("\n");
    const node = {
      content,
      type: NodeTypes.COMMENT,
      loc: {
        source: `<!-- ${content} -->`,
      },
    } as CommentNode;

    expect(handleComment(node)).toMatchObject({
      content: `${content}`,
      node,
    });
  });
});
