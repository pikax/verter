import { declareTemplatePlugin } from "../../template";

const IgnoredASTTypes = new Set([
  "MemberExpression",
  "ObjectExpression",
  "ArrowFunctionExpression",
  "FunctionExpression",
]);

export const EventPlugin = declareTemplatePlugin({
  name: "VerterPropEvent",

  /**
   * 
   * 
declare function ___VERTER___eventCb
    <TArgs extends Array<any>, R extends ($event: TArgs[0]) => any>(event: TArgs, cb: R): R;

   */

  transformProp(prop, s, ctx) {
    if (!prop.event) return;

    const { exp } = prop.node;

    if (!exp || !exp.ast) {
      return;
    }

    if (IgnoredASTTypes.has(exp.ast.type)) {
      return;
    }

    const eventCb = ctx.retrieveAccessor("eventCb");
    const eventArgs = ctx.retrieveAccessor("eventArgs");

    s.prependLeft(
      exp.loc.start.offset,
      `(...${eventArgs})=>${eventCb}(${eventArgs},($event)=>$event&&0?undefined:`
    );

    s.prependLeft(exp.loc.end.offset, ")");
  },
});
