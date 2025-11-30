import { ParseTemplateContext } from "../../../../parser/template";
import { createHelperImport } from "../../../utils";
import { declareTemplatePlugin } from "../../template";

const IgnoredASTTypes = new Set([
  "MemberExpression",
  "ObjectExpression",
  "ArrowFunctionExpression",
  "FunctionExpression",
]);

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

    if (IgnoredASTTypes.has(exp.ast.type)) {
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
