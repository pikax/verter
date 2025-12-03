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
import { patchAcornNodeLoc, patchBabelNodeLoc } from "../../utils/node/node.js";
import { BindingPattern, VerterASTNode } from "../ast/types.js";
import { parseAcornLoose } from "../ast/ast.js";
import type AcornTypes from "acorn";

const Keywords =
  "break,case,catch,class,const,continue,debugger,default,delete,do,else,export,extends,false,finally,for,function,if,import,in,instanceof,new,null,return,super,switch,this,throw,true,try,typeof,var,void,while,with,await".split(
    ","
  );
// only in strict
const KeywordsInStrict = ["let", "static", "yield"];

const Literals = "undefined,null,true,false".split(",");

const BindingIgnoreArray = [...Keywords, ...KeywordsInStrict, ...Literals];
const BindingIgnoreSet = new Set(BindingIgnoreArray);

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
      ignore:
        context.ignoredIdentifiers.includes(name) ||
        BindingIgnoreSet.has(name) ||
        exp.isStatic,
      directive,
      exp,
    });
  } else if (exp.ast) {
    bindings.push(...getASTBindings(exp.ast, context, exp));
  } else {
    const ast = parseAcornLoose(exp.content);
    if (ast) {
      bindings.push(...getASTBindings(ast, context, exp));
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
  }

  return bindings;
}

export function getASTBindings(
  ast: babel_types.Node | VerterASTNode | AcornTypes.Program,
  context: Record<string, any>,
  exp?: Node
) {
  const bindings = [] as Array<
    TemplateBinding | TemplateFunction | TemplateLiteral
  >;

  const isAcorn = "type" in ast && ast.type === "Program";

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
          // Collect function parameters
          const params = n.params;
          params.forEach((param) => {
            // Collect all parameter identifiers including rest parameters
            collectDeclaredIds(param as VerterASTNode, ignoredIdentifiers);
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
        case "ArrowFunctionExpression": {
          const params = n.params;
          params.forEach((param) => {
            // Collect all parameter identifiers including rest parameters
            collectDeclaredIds(param as VerterASTNode, ignoredIdentifiers);
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
        case "VariableDeclaration": {
          const toAdd = [] as string[];
          // @ts-expect-error it's correct
          n.declarations.forEach((x) => collectDeclaredIds(x.id, toAdd));

          // collectDeclaredIds(n.id, toAdd);
          if (ignoredIdentifiers) {
            ignoredIdentifiers.push(...toAdd);
          }
          // @ts-expect-error not typed
          if (parent?._ignoredIdentifiers) {
            // @ts-expect-error not typed
            parent!._ignoredIdentifiers.push(...toAdd);
          }
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
          // this is the node where acorn fails
          if (isAcorn && name === "✖") {
            this.skip();
            return;
          }
          const pNode = exp
            ? isAcorn
              ? // @ts-expect-error not correct type
                patchAcornNodeLoc(n, exp)
              : patchBabelNodeLoc(n, exp)
            : n;

          bindings.push({
            type: TemplateTypes.Binding,
            // @ts-expect-error not correct type
            node: pNode,
            name,
            // @ts-expect-error not correct type
            parent,
            directive: null,

            ignore:
              ignoredIdentifiers.includes(name) || BindingIgnoreSet.has(name),
            exp: exp as SimpleExpressionNode | null,
          });
          break;
        }
        default: {
          if (n.type.endsWith("Literal")) {
            // @ts-expect-error not correct type
            const pNode = exp && !isAcorn ? patchBabelNodeLoc(n, exp) : n;
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
              // @ts-expect-error
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

// Recursively collect all declared identifiers from the pattern
function collectDeclaredIds(node: VerterASTNode, ignoredIdentifiers: string[]) {
  switch (node.type) {
    case "Identifier":
      ignoredIdentifiers.push(node.name);
      break;
    case "RestElement":
      if (node.argument.type === "Identifier") {
        ignoredIdentifiers.push(node.argument.name);
      } else {
        collectDeclaredIds(node.argument, ignoredIdentifiers);
      }
      break;
    case "AssignmentPattern":
      collectDeclaredIds(node.left, ignoredIdentifiers);
      break;
    case "ArrayPattern":
      for (const elem of node.elements) {
        if (elem) collectDeclaredIds(elem, ignoredIdentifiers);
      }
      break;
    case "ObjectPattern":
      for (const prop of node.properties) {
        if (prop.type === "Property") {
          // { key: value } or shorthand
          collectDeclaredIds(prop.value, ignoredIdentifiers);
        } else if (prop.type === "RestElement") {
          collectDeclaredIds(prop.argument, ignoredIdentifiers);
        }
      }
      break;
    // add more cases if you need to handle TS-specific patterns, etc.
  }
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
