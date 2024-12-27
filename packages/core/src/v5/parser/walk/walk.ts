import type { VerterAST, Statement, ModuleDeclaration } from "../ast/index.js";

export function shallowWalk(
  root: VerterAST,
  cb: (node: Statement | ModuleDeclaration) => void
) {
  for (let i = 0; i < root.body.length; i++) {
    cb(root.body[i]);
  }
}
