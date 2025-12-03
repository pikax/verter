import { ParseTemplateContext } from "../../../../parser/template";
import { createHelperImport } from "../../../utils";
import { declareTemplatePlugin } from "../../template";

const IgnoredASTTypes = new Set([
  "MemberExpression",
  "ObjectExpression",
  "ArrowFunctionExpression",
  "FunctionExpression",
]);


/**
 * Determines if an event handler expression should skip the event callback wrapping.
 * 
 * This function handles two cases:
 * 1. Direct function expressions: @click="()=>test", @click="function(){}"
 * 2. Block-bodied functions: @click="(event) => { event; }"
 * 
 * The second case is parsed as a Program node by the Vue compiler, so we need to
 * check if the Program contains a single ExpressionStatement with a function.
 * 
 * @param ast - The AST node of the event handler expression
 * @returns true if the event wrapping should be skipped
 */
function shouldSkipEventWrapping(ast: any): boolean {
  if (IgnoredASTTypes.has(ast.type)) {
    return true;
  }
  
  // Check if it's a Program with a single statement that is a function
  // This handles cases like: @click="(event) => { event; }"
  if (ast.type === "Program" && ast.body && ast.body.length === 1) {
    const statement = ast.body[0];
    // Check if it's an ExpressionStatement containing a function
    if (statement.type === "ExpressionStatement" && statement.expression) {
      const exprType = statement.expression.type;
      return exprType === "ArrowFunctionExpression" || exprType === "FunctionExpression";
    }
  }
  
  return false;
}

export const EventPlugin = declareTemplatePlugin({
  name: "VerterPropEvent",

  inject: false,
  // pre() {
  //   this.inject = false;
  // },
  /**
   * 
declare function ___VERTER___eventCb<TArgs extends Array<any>, R extends ($event: TArgs[0]) => any>(event: TArgs, cb: R): R;
   */

  // post(s, ctx) {
  //   if (!this.inject) return;
  //   s.append(
  //     "declare function ___VERTER___eventCb<TArgs extends Array<any>, R extends ($event: TArgs[0]) => any>(event: TArgs, cb: R): R;"
  //   );
  // },

  transformProp(prop, s, ctx) {
    if (!prop.event) return;

    const { exp } = prop.node;

    if (!exp || !exp.ast) {
      return;
    }

    if (shouldSkipEventWrapping(exp.ast)) {
      return;
    }
    ctx.items.push(createHelperImport(["eventCallbacks"], ctx.prefix));

    const eventCallbacks = ctx.retrieveAccessor("eventCallbacks");
    const eventArgs = ctx.retrieveAccessor("eventArgs");

    s.prependLeft(
      exp.loc.start.offset,
      `(...${eventArgs})=>${eventCallbacks}(${eventArgs},($event)=>$event&&0?undefined:`
    );

    const context = prop.context as ParseTemplateContext;
    if (ctx.doNarrow && context.conditions.length > 0) {
      ctx.doNarrow(
        {
          index: exp.loc.start.offset,
          inBlock: false,
          conditions: context.conditions,
          type: "append",
        },
        s
      );
    }

    s.prependLeft(exp.loc.end.offset, ")");
  },
});
