import {
  DirectiveNode,
  Node,
  NodeTypes,
  SimpleExpressionNode,
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
  TemplateLiteral,
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
import { VerterASTNode } from "../ast/types.js";

export function retrieveBindings(
  exp: ExpressionNode,
  context: {
    ignoredIdentifiers: string[];
  },
  directive: null | DirectiveNode = null
): Array<TemplateBinding | TemplateFunction | TemplateLiteral> {
  const bindings: Array<TemplateBinding | TemplateFunction | TemplateLiteral> =
    [];

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
      exp,
    });
  } else if (exp.ast) {
    bindings.push(...getASTBindings(exp.ast, context, exp));
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
      exp,
    });
  }

  return bindings;
}

export function getASTBindings(
  ast: babel_types.Node | VerterASTNode,
  context: Record<string, any>,
  exp?: Node
) {
  const bindings = [] as Array<
    TemplateBinding | TemplateFunction | TemplateLiteral
  >;
  walk(ast, {
    enter(
      n: babel_types.Node | VerterASTNode,
      parent: babel_types.Node | VerterASTNode
    ) {
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
          const params = Array.isArray(n.params) ? n.params : n.params.items;
          params.forEach((param) => {
            if (param.type === "Identifier") {
              ignoredIdentifiers.push(param.name);
            }
          });

          const pN = exp ? patchBabelNodeLoc(n as babel_types.Node, exp) : n;
          const bN = exp
            ? patchBabelNodeLoc(n.body as babel_types.Node, exp)
            : n;
          bindings.push({
            type: TemplateTypes.Function,
            // @ts-expect-error not correct type
            node: pN,
            // @ts-expect-error not correct type
            body: bN,
            context,
          });
          break;
        }
        case "Identifier": {
          const name = n.name;
          if (
            parent &&
            (("property" in parent && parent.property === n) ||
              ("key" in parent && parent.key === n))
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
          // @ts-expect-error not correct type
          const pNode = exp ? patchBabelNodeLoc(n, exp) : n;

          bindings.push({
            type: TemplateTypes.Binding,
            // @ts-expect-error not correct type
            node: pNode,
            name,
            // @ts-expect-error not correct type
            parent,
            directive: null,

            ignore: ignoredIdentifiers.includes(name),
            exp: exp as SimpleExpressionNode | null,
          });
          break;
        }
        default: {
          if (n.type.endsWith("Literal")) {
            // @ts-expect-error not correct type
            const pNode = exp ? patchBabelNodeLoc(n, exp) : n;
            let content = "";
            let value = undefined;

            if ("value" in n) {
              content = `${n.value}`;
              value = n.value;
            }

            bindings.push({
              type: TemplateTypes.Literal,
              content,
              value,
              node: pNode,
            });
          }
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
    [TemplateTypes.Literal]: [] as Array<TemplateLiteral>,
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
