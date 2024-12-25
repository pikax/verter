import type { VerterAST } from "../ast/index.js";
import type { Statement, ModuleDeclaration } from "acorn";

export function shallowWalk(
  root: VerterAST,
  cb: (node: Statement | ModuleDeclaration) => void
) {
  for (let i = 0; i < root.body.length; i++) {
    cb(root.body[i]);
  }
}
