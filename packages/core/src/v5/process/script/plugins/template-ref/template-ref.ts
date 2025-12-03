import { definePlugin, ScriptContext } from "../../types";
import { CallExpression, ObjectProperty } from "../../../../parser/ast/types";
import { MagicString } from "@vue/compiler-sfc";
import {
  TemplateElement,
  TemplateTypes,
} from "../../../../parser/template/types";
import {
  DirectiveNode,
  ElementTypes,
  Node,
  SimpleExpressionNode,
} from "@vue/compiler-core";
import { camelize, capitalize, HTMLAttributes } from "vue";
import { ScriptDeclaration, ScriptTypes } from "../../../../parser/script";
import { isHTMLTag } from "@vue/shared";
import * as babel_types from "@babel/types";

const NormalisedComponentsAccessor = "NormalisedComponents";

export const TemplateRefPlugin = definePlugin({
  name: "VerterTemplateRef",

  transformDeclaration(item, s, ctx) {
    if (
      item.parent.type === "VariableDeclarator" &&
      item.parent.init?.type === "CallExpression" &&
      item.parent.init.typeArguments === null
    ) {
      if (item.parent.init.callee.type === "Identifier") {
        const macroName = item.parent.init.callee.name;
        if (macroName !== "ref" || !item.name) {
          return;
        }
        const varName = item.name;

        handleExpression(item.parent.init, s, varName, ctx);
      }
    }
  },

  transformFunctionCall(item, s, ctx) {
    if (item.name !== "useTemplateRef") return;
    handleExpression(item.node, s, undefined, ctx);
  },
});

function handleExpression(
  node: CallExpression,
  s: MagicString,
  _name: string | undefined,
  ctx: ScriptContext
) {
  // don't handle if there's explicit type parameters
  if (node.typeArguments !== null) return;
  const templateItems = ctx.blocks
    .find((x) => x.type === "template")
    ?.result?.items.filter((x) => x.type === TemplateTypes.Element);

  if (!templateItems || !templateItems.length) {
    return;
  }

  const nameArg = node.arguments?.[0];

  const name =
    _name ??
    (nameArg
      ? nameArg.type === "Literal"
        ? s.original.slice(nameArg.start + 1, nameArg.end - 1)
        : s.original.slice(nameArg.start, nameArg.end)
      : "");

  const possibleTypes = [] as string[];
  const possibleNames = [] as string[];

  for (const item of templateItems) {
    if (!item.ref) continue;
    const ref = item.ref;
    const rawRefName =
      "arg" in ref
        ? // @ts-expect-error
          ref.node.exp?.ast?.type &&
          // @ts-expect-error
          ref.node.exp?.ast?.type?.indexOf("Function") !== -1
          ? ""
          : `typeof ${ref.exp?.[0]?.exp?.content}`
        : // @ts-expect-error
          `"${ref.value}"`;

    if (!rawRefName) continue;
    possibleNames.push(rawRefName);
    const refName = rawRefName.startsWith("typeof ")
      ? rawRefName.slice(7)
      : rawRefName;

    if (!name || name === refName || ("value" in ref && name === ref.value)) {
      const tag = resolveTagNameType(item, ctx);
      if (Array.isArray(tag)) {
        possibleTypes.push(...tag);
      } else if (tag) {
        possibleTypes.push(tag);
      }
    } else if (ctx.block.result?.items) {
      for (const scriptItem of ctx.block.result.items) {
        if (scriptItem.type !== ScriptTypes.Declaration) continue;
        let declarationValue: string | undefined =
          retrieveDeclarationStringValue(scriptItem);
        if (!declarationValue) {
          if (refName.indexOf(".")) {
            declarationValue = retrieveDeclarationStringValueFromObject(
              scriptItem,
              refName
            );
          }
        }
        if (
          declarationValue === name &&
          (~refName.indexOf(".") || scriptItem.name === refName)
        ) {
          const tag = resolveTagNameType(item, ctx);
          if (Array.isArray(tag)) {
            possibleTypes.push(...tag);
          } else if (tag) {
            possibleTypes.push(tag);
          }
        }
      }
    }
  }
  if (possibleNames.length === 0) return;

  const types = possibleTypes.join("|");
  const names = possibleNames.join("|");

  // TODO if the name is a typeof of the current variable it should log an error
  /** eg:
   * const a = useTemplateRef();
   * <div :ref="a" />
   */

  const isTS = ctx.block.lang.startsWith("ts");

  if (isTS) {
    if (_name) {
      if (!types) return;
      s.prependLeft(node.callee.end, `<${types}|null>`);
    } else {
      s.prependLeft(node.callee.end, `<${types || "unknown"},${names}>`);
    }
  } else {
    if (_name) {
      if (!types) return;
      s.prependLeft(
        node.callee.start,
        `/**@type{typeof import('vue').ref<${types}|null>}*/(`
      );
    } else {
      s.prependLeft(
        node.callee.start,
        `/**@type{typeof import('vue').useTemplateRef<${
          types || "unknown"
        },${names}>}*/(`
      );
    }
    s.prependLeft(node.callee.end, ")");
  }
}

function resolveTagNameType(
  item: TemplateElement,
  ctx: ScriptContext
): string | string[] | undefined {
  if (item.tag === "component") {
    const propIs = item.props?.find(
      (x) =>
        x.name === "is" ||
        (x.type === TemplateTypes.Prop &&
          "arg" in x &&
          x.arg?.[0]?.name === "is")
    );
    if (!propIs) return;
    if (propIs.static && propIs.value) {
      return resolveTagNameForTag(propIs.value, ctx);
    }
    const directive = propIs.node as DirectiveNode;
    const exp = directive.exp as SimpleExpressionNode;
    if (!exp) return;
    if (exp.ast) {
      const leafs = findAstConditionsLeafs(exp.ast);

      // TODO support other types
      return leafs
        .filter((x) => x.type === "StringLiteral")
        .map((x) => resolveTagNameForTag(x.value, ctx));
    } else {
      // todo helper type to extract the actual type
      return `typeof ${exp.content}`;
    }
  }

  return `ReturnType<typeof ${ctx.prefix("Comp")}${item.node.loc.start.offset}${
    ctx.generic ? `<${ctx.generic.names.join(",")}>` : ""
  }>${item.context.inFor ? "[]" : ""}`;
}

function resolveTagNameForTag(tag: string, ctx: ScriptContext) {
  const Normalised = ctx.prefix(NormalisedComponentsAccessor);
  if (isHTMLTag(tag)) {
    return `HTML${capitalize(tag)}Element`;
  }

  if (~tag.indexOf(".")) {
    const newTag = tag
      .split(".")
      .map((x) => `["${x}"]`)
      .join("");

    return `${Normalised}${newTag}`;
  }

  const camel = camelize(tag);
  const camelCapitalised = capitalize(camel);

  //   const found = {} as Record<string, string>;
  const found = new Set<string>();
  if (ctx.block.result?.items) {
    for (const item of ctx.block.result?.items) {
      let names: string | Array<string> | undefined = undefined;
      if (item.type === ScriptTypes.Binding) {
        names = [item.name];
      } else if (item.type === ScriptTypes.Import) {
        names = item.bindings.map((x) => x.name);
      } else if (item.type === ScriptTypes.Declaration) {
        const vname = retrieveDeclarationStringValue(item);
        if (vname) {
          names = [vname];
        }
      }

      if (!names) continue;
      names.forEach((name) => {
        if (name === tag) {
          found.add(name);
        }
        if (name === camel) {
          found.add(name);
        }
        if (name === camelCapitalised) {
          found.add(name);
        }
      });
    }
  }

  let t = "";
  if (found.has(tag)) {
    t = tag;
  } else if (found.has(camel)) {
    t = camel;
  } else if (found.has(camelCapitalised)) {
    t = camelCapitalised;
  }

  if (!t) return `${Normalised}["${tag}"]`;
  return `${Normalised}["${t}"]`;
}

function retrieveDeclarationStringValue(item: ScriptDeclaration) {
  if (item.parent.type !== "VariableDeclarator") return;
  const init = item.parent.init;
  if (!init) return;
  if (init.type === "Literal") {
    return init.value?.toString();
  }
  if (init.type === "TemplateLiteral") {
    return init.quasis[0].value.raw;
  }
  return;
}

function retrieveDeclarationStringValueFromObject(
  item: ScriptDeclaration,
  path: string
) {
  if (item.parent.type !== "VariableDeclarator") return undefined;
  const init = item.parent.init;
  if (!init || init.type !== "ObjectExpression") return undefined;

  const parts = path.split(".");

  if (item.name !== parts.shift()) return undefined;

  let object = init;
  while (parts.length) {
    const part = parts.shift();
    if (!part) break;
    const property = object.properties.find(
      (x) =>
        x.type === "Property" &&
        x.key.type === "Identifier" &&
        x.key.name === part
    ) as ObjectProperty | undefined;
    if (!property) break;

    if (parts.length === 0) {
      return property.value.type === "Literal"
        ? property.value.value?.toString()
        : "expression" in property.value &&
          typeof property.value.expression === "object" &&
          "value" in property.value.expression
        ? property.value.expression?.value?.toString()
        : "";
    }

    // init = findProperty(init, part);
  }
  return undefined;

  //   if (init.type === "ObjectExpression") {
  //     for (const property of init.properties) {
  //       if (property.key.type === "Identifier" && property.key.name === path) {
  //         return property.value.type === "Literal"
  //           ? property.value.value?.toString()
  //           : property.value.value?.toString();
  //       }
  //     }
  //   }
}

function findAstConditionsLeafs(node: babel_types.Node) {
  const leafs = [] as babel_types.Node[];
  const queue = [node];
  while (queue.length) {
    const n = queue.shift();
    if (!n) continue;
    if (n.type === "ConditionalExpression") {
      queue.push(n.consequent, n.alternate);
    } else if (n.type === "LogicalExpression") {
      queue.push(n.left, n.right);
    } else if (n.type === "BinaryExpression") {
      queue.push(n.left, n.right);
    } else {
      leafs.push(n);
    }
  }
  return leafs;
}
