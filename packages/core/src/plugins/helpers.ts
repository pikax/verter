import { Node, Statement } from "@babel/types";
import { VueAPISetup } from "./types.js";

export function* checkForSetupMethodCalls(
  name: VueAPISetup,
  statement: Statement
) {
  if (statement.type === "ExpressionStatement") {
    if (
      statement.expression.type === "CallExpression" &&
      "name" in statement.expression.callee &&
      statement.expression.callee.name === name
    ) {
      yield statement.expression;
    }
  } else if (
    statement.type === "VariableDeclaration" &&
    statement.declarations &&
    statement.declarations.length
  ) {
    for (let d = 0; d < statement.declarations.length; d++) {
      const declaration = statement.declarations[d];
      if (
        declaration?.init?.type === "CallExpression" &&
        "name" in declaration.init.callee &&
        declaration.init.callee.name === name
      ) {
        yield declaration.init;
      }
    }
  }
}

export function checkForSetupMethodCall(
  name: VueAPISetup,
  statement: Statement
) {
  return checkForSetupMethodCalls(name, statement).next().value;
}

export function retrieveNodeString(
  node: Node | undefined | null,
  source: string
) {
  if (!node) return undefined;
  return source.slice(node.start ?? 0, node.end ?? -1);
}
