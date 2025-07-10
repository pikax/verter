import type { Node, Position } from "@vue/compiler-core";
import type * as babel_types from "@babel/types";
import type AcornTypes from "acorn";

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
export function patchAcornPosition(
  pos: AcornTypes.SourceLocation["start"],
  offsetPos: Position
) {
  pos.line = offsetPos.line + pos.line - 1;
  pos.column = offsetPos.column + pos.column - 1;
  pos.offset =
    // @ts-expect-error not part of pos
    offsetPos.offset + (pos.index == null ? pos.offset : pos.index - 1);

  return pos as babel_types.SourceLocation["start"] & Position;
}

export function patchAcornNodeLoc<T extends AcornTypes.AnyNode>(
  node: T,
  templateNode: Node
) {
  if (node.loc) {
    patchAcornPosition(node.loc.start, templateNode.loc.start);
    patchAcornPosition(node.loc.end, templateNode.loc.start);

    // @ts-expect-error not part of pos
    const start = node.loc?.start.offset;
    // @ts-expect-error not part of pos
    const end = node.loc?.end.offset;

    // @ts-expect-error not part of pos
    node.loc.source = templateNode.content.slice(start, end);
  } else {
    node.loc = {
      start: { line: 0, column: 0, offset: node.start },
      end: { line: 0, column: 0, offset: node.end },
      // @ts-expect-error not part of loc
      source: templateNode.content.slice(node.start, node.end),
    } as any;
    patchAcornPosition(node.loc!.start, templateNode.loc.start);
    patchAcornPosition(node.loc!.end, templateNode.loc.start);
  }

  return node as AcornTypes.AnyNode & {
    loc: Node["loc"] & AcornTypes.AnyNode["loc"];
  };
}
