import { NodeTypes, TextNode } from "@vue/compiler-core";
import { handleText } from "./index.js";

describe("parser template text", () => {
  it("should return text", () => {
    const node = {
      content: "text",
      type: NodeTypes.TEXT,
      loc: {
        source: "text",
      },
    } as TextNode;

    expect(handleText(node)).toMatchObject({
      content: "text",
      node,
    });
  });

  it("should return null if node type is not 'TEXT'", () => {
    const node = {
      content: "text",
      loc: {
        source: "text",
      },
    };

    expect(handleText(node as any)).toBeNull();
  });
});
