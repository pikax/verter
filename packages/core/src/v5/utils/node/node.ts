import { Node, Position } from "@vue/compiler-core";
import * as babel_types from "@babel/types";
export function patchBabelNodeLoc<T extends babel_types.Node>(
  node: T,
  templateNode: Node
) {
  if (node.loc) {
    patchBabelPosition(node.loc.start, templateNode.loc.start);
    patchBabelPosition(node.loc.end, templateNode.loc.start);

    // @ts-expect-error not part of loc
    node.loc.source =
      node.loc.identifierName ??
      // @ts-expect-error not part of loc
      templateNode.content.slice(
        node.loc.start.index - 1,
        node.loc.end.index - 1
      );
  }

  return node as babel_types.Node & {
    loc: Node["loc"] & babel_types.Node["loc"];
  };
}

export function patchBabelPosition(
  pos: babel_types.SourceLocation["start"],
  offsetPos: Position
) {
  pos.line = offsetPos.line + pos.line - 1;
  pos.column = offsetPos.column + pos.column - 1;
  // @ts-expect-error not part of pos
  pos.offset = offsetPos.offset + pos.index - 1;

  return pos as babel_types.SourceLocation["start"] & Position;
}
