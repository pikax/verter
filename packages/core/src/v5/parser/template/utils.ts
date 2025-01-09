import {
  DirectiveNode,
  NodeTypes,
  type ExpressionNode,
} from "@vue/compiler-core";
import {
  TemplateComment,
  TemplateCondition,
  TemplateDirective,
  TemplateElement,
  TemplateFunction,
  TemplateInterpolation,
  TemplateItem,
  TemplateItemByType,
  TemplateLoop,
  TemplateProp,
  TemplateRenderSlot,
  TemplateSlot,
  TemplateText,
  TemplateTypes,
  type TemplateBinding,
} from "./types.js";
import { walk } from "@vue/compiler-sfc";
import * as babel_types from "@babel/types";
import { patchBabelNodeLoc } from "../../utils/node/node.js";

export function retrieveBindings(
  exp: ExpressionNode,
  context: {
    ignoredIdentifiers: string[];
  },
  directive: null | DirectiveNode = null
): Array<TemplateBinding | TemplateFunction> {
  const bindings: Array<TemplateBinding | TemplateFunction> = [];

  if (exp.type !== NodeTypes.SIMPLE_EXPRESSION) {
    return bindings;
  }
  if (exp.isStatic || exp.ast === null) {
    const name = exp.content;
    bindings.push({
      type: TemplateTypes.Binding,
      node: exp,
      name,
      parent: null,
      ignore: context.ignoredIdentifiers.includes(name) || exp.isStatic,
      directive,
    });
  } else if (exp.ast) {
    walk(exp.ast, {
      enter(n: babel_types.Node, parent: babel_types.Node) {
        const ignoredIdentifiers: string[] =
          // @ts-expect-error
          (n._ignoredIdentifiers = [
            // @ts-expect-error
            ...(parent?._ignoredIdentifiers || context.ignoredIdentifiers),
          ]);

        switch (n.type) {
          case "FunctionDeclaration":
          case "FunctionExpression": {
            if (n.id) {
              ignoredIdentifiers.push(n.id.name);
            }
          }
          case "ArrowFunctionExpression": {
            n.params.forEach((param) => {
              if (param.type === "Identifier") {
                ignoredIdentifiers.push(param.name);
              }
            });

            const pN = patchBabelNodeLoc(n, exp);
            const bN = patchBabelNodeLoc(n.body, exp);
            bindings.push({
              type: TemplateTypes.Function,
              node: pN,
              body: bN,
              context,
            });
            break;
          }
          case "Identifier": {
            const name = n.name;
            if (
              ("property" in parent && parent.property === n) ||
              ("key" in parent && parent.key === n)
            ) {
              this.skip();
              return;
            }
            // if (
            //   (parent.type === "OptionalMemberExpression" &&
            //     parent.property === n) ||
            //   (parent.type === "MemberExpression" && parent.property === n) ||
            //   (parent.type === "ObjectProperty" && parent.key === n) ||
            //   (parent.type === "ClassMethod" && parent.key === n)
            // ) {
            //   this.skip();
            //   return;
            // }
            const pNode = patchBabelNodeLoc(n, exp);

            bindings.push({
              type: TemplateTypes.Binding,
              node: pNode,
              name,
              parent,
              directive:null,

              ignore: ignoredIdentifiers.includes(name),
            });
            break;
          }
          default: {
            if ("id" in n) {
              if (n.id?.type === "Identifier") {
                ignoredIdentifiers.push(n.id.name);
              }
            }
          }
        }
      },

      leave(n: babel_types.Node) {
        // @ts-expect-error
        delete n._ignoredIdentifiers;
      },
    });
  } else {
    bindings.push({
      type: TemplateTypes.Binding,
      node: exp,
      context,

      value: exp.content,
      invalid: true,
      ignore: false,
      name: undefined,
      parent: null,
    });
  }

  return bindings;
}

export function createTemplateTypeMap() {
  return {
    [TemplateTypes.Condition]: [] as Array<TemplateCondition>,
    [TemplateTypes.Loop]: [] as Array<TemplateLoop>,
    [TemplateTypes.Element]: [] as Array<TemplateElement>,
    [TemplateTypes.Prop]: [] as Array<TemplateProp>,
    [TemplateTypes.Binding]: [] as Array<TemplateBinding>,
    [TemplateTypes.SlotRender]: [] as Array<TemplateRenderSlot>,
    [TemplateTypes.SlotDeclaration]: [] as Array<TemplateSlot>,
    [TemplateTypes.Comment]: [] as Array<TemplateComment>,
    [TemplateTypes.Text]: [] as Array<TemplateText>,
    [TemplateTypes.Directive]: [] as Array<TemplateDirective>,
    [TemplateTypes.Interpolation]: [] as Array<TemplateInterpolation>,
    [TemplateTypes.Function]: [] as Array<TemplateFunction>,
  } satisfies {
    [K in TemplateTypes]: Array<TemplateItemByType[K]>;
  };
}

export function templateItemsToMap(items: TemplateItem[]) {
  const map = createTemplateTypeMap();

  for (const item of items) {
    map[item.type].push(item as any);
  }

  return map;
}
