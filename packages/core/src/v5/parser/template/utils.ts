import {
  DirectiveNode,
  Node,
  NodeTypes,
  SimpleExpressionNode,
  type ExpressionNode,
} from "@vue/compiler-core";
import {
  TemplateBrokenExpression,
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
import { parseAcornLoose, parseOXC } from "../ast/ast.js";
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
): Array<
  | TemplateBinding
  | TemplateFunction
  | TemplateLiteral
  | TemplateBrokenExpression
> {
  const bindings: Array<
    | TemplateBinding
    | TemplateFunction
    | TemplateLiteral
    | TemplateBrokenExpression
  > = [];

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
    patchBabelNodeLoc(exp.ast as babel_types.Node, exp);
    bindings.push(...getASTBindings(exp.ast, context, exp));
  } else if (exp.content.trim() !== "") {
    const ast = parseOXC(exp.content)?.program?.body[0];
    if (ast) {
      bindings.push(...getASTBindings(ast, context, exp));
    } else {
      bindings.push({
        type: TemplateTypes.BrokenExpression,
        node: exp,
        directive,
      });
    }

    // const ast = parseAcornLoose(exp.content);
    // if (ast) {
    //   bindings.push(...getASTBindings(ast, context,  exp));
    // } else {
    //   bindings.push({
    //     type: TemplateTypes.Binding,
    //     node: exp,
    //     context,

    //     value: exp.content,
    //     invalid: true,
    //     ignore: false,
    //     name: undefined,
    //     parent: null,
    //     exp,
    //   });
    // }
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
      parent: babel_types.Node | VerterASTNode,
      key: string | number | null
    ) {
      const inheritedIgnoreBindings =
        // @ts-expect-error internal flag used at runtime
        parent?._ignoreBindings === true;

      // @ts-expect-error internal flag used at runtime
      n._ignoreBindings = inheritedIgnoreBindings;

      if (
        !inheritedIgnoreBindings &&
        n.type === "ObjectExpression" &&
        parent &&
        (parent.type === "TSAsExpression" ||
          parent.type === "TSSatisfiesExpression")
      ) {
        // Object literals used purely in type assertions/satisfies should not contribute bindings
        // @ts-expect-error internal flag used at runtime
        n._ignoreBindings = true;
      }

      const ignoredIdentifiers: string[] =
        // @ts-expect-error
        (n._ignoredIdentifiers = [
          // @ts-expect-error
          ...(parent?._ignoredIdentifiers || context.ignoredIdentifiers),
        ]);

      if (parent) {
        if (
          (parent.type === "ClassMethod" && key === "key") ||
          (parent.type === "ObjectProperty" && key === "key") ||
          (parent.type === "MemberExpression" &&
            key === "property" &&
            (!exp ||
              ("content" in exp &&
                typeof exp.content === "string" &&
                exp.content[
                  n.start! -
                    2 /* the AST always starts at 1 and we want the previous */
                ]) !== "[")) ||
          (parent.type === "TSTypeReference" && key === "typeName")
        ) {
          this.skip();
          return;
        }
      }

      switch (n.type) {
        case "FunctionDeclaration":
        case "FunctionExpression": {
          if (n.id) {
            ignoredIdentifiers.push(n.id.name);
          }
        }
        case "ArrowFunctionExpression": {
          const params = n.params;
          params.forEach((param) => {
            if (param.type === "Identifier") {
              ignoredIdentifiers.push(param.name);
            } else if (
              param.type === "RestElement" &&
              param.argument.type === "Identifier"
            ) {
              ignoredIdentifiers.push(param.argument.name);
            } else if (param.type === "AssignmentPattern") {
              const left = param.left;
              if (left.type === "Identifier") {
                ignoredIdentifiers.push(left.name);
              }
            } else if (
              param.type === "ObjectPattern" ||
              param.type === "ArrayPattern"
            ) {
              collectDeclaredIds(param as BindingPattern, ignoredIdentifiers);
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
          // @ts-expect-error internal flag used at runtime
          if (n._ignoreBindings === true) {
            this.skip();
            return;
          }

          const name = n.name;
          if (
            parent &&
            (parent.type === "ObjectProperty" || parent.type === "Property") &&
            (parent as babel_types.ObjectProperty | babel_types.ObjectMethod)
              .key === n
          ) {
            this.skip();
            return;
          }
          // console.log("Identifier:", name, ast);
          if (
            exp &&
            parent &&
            (("property" in parent && parent.property === n) ||
              ("key" in parent && parent.key === n))
          ) {
            const content =
              exp?.type === NodeTypes.SIMPLE_EXPRESSION
                ? // @ts-expect-error TODO Fix
                  exp.content.slice(
                    parent.start! - ast.start!,
                    parent.end! - ast.start!
                  )
                : (parent as any).loc.source;

            const accessor = content[n.start! - parent.start! - 1];

            if (accessor === ".") {
              this.skip();
              return;
            }
          }

          // if (
          //   parent &&
          //   (("property" in parent &&
          //     parent.property === n &&
          //     !(
          //       (parent as any).extra?.parenthesized === true &&
          //       parent.loc.source[(parent as any).extra.parenStart] === "["
          //     )) ||
          //     ("key" in parent && parent.key === n))
          // ) {
          //   this.skip();
          //   return;
          // }
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
        // case "ObjectProperty": {
        //   if (n.key.type === "Identifier") {
        //     ignoredIdentifiers.push(n.key.name);
        //   }
        //   break;
        // }
        case "TSPropertySignature": {
          if (n.key.type === "Identifier") {
            ignoredIdentifiers.push(n.key.name);
          }
          break;
        }
        // case "ClassExpression": {
        //   if (n.id && n.id.type === "Identifier") {
        //     bindings.push({
        //       type: TemplateTypes.Binding,
        //       // @ts-expect-error not correct type
        //       node: exp ? patchBabelNodeLoc(n.id, exp) : n.id,
        //       name: n.id.name,
        //       parent: null,
        //       ignore: true,
        //       directive: null,
        //       exp: exp as SimpleExpressionNode | null,
        //     });

        //     ignoredIdentifiers.push(n.id.name);
        //   }
        //   // this.skip();
        //   break;
        // }
        default: {
          if (n.type.endsWith("Literal") && !n.type.startsWith("TS")) {
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
              // @ts-expect-error not correct type
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
      // @ts-expect-error
      delete n._ignoreBindings;
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
        collectDeclaredIds(prop, ignoredIdentifiers);
        // if (prop.type === "Property") {
        //   // { key: value } or shorthand
        //   collectDeclaredIds(prop.value, ignoredIdentifiers);
        // } else if (prop.type === "RestElement") {
        //   collectDeclaredIds(prop.argument, ignoredIdentifiers);
        // } else if (prop.type === "ObjectProperty") {
        //   collectDeclaredIds(prop.value, ignoredIdentifiers);
        // }
      }
      break;
    case "Property": {
      collectDeclaredIds(node.value, ignoredIdentifiers);
      break;
    }
    // @ts-expect-error TODO fix
    case "ObjectProperty": {
      // @ts-expect-error TODO fix
      collectDeclaredIds(node.value, ignoredIdentifiers);
      break;
    }
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
    [TemplateTypes.BrokenExpression]: [] as Array<TemplateBrokenExpression>,
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
