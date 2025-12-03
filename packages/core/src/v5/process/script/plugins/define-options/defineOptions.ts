// // moving before the template
//     const item = ctx.items.find((x) => x.type === ProcessItemType.Options);
//     if (item) {
//       s.move(
//         item.node.start,
//         item.node.end,
//         // Math.max(...ctx.blocks.map((x) => x.block.tag.pos.close.end))
//         -1
//       );
//     }
import { definePlugin } from "../../types";
import type {
  CallExpression,
  VerterASTNode,
} from "../../../../parser/ast/types";
import { ProcessContext, ProcessItemType } from "../../../types";
import { MagicString } from "@vue/compiler-sfc";
import { createHelperImport } from "../../../utils";
import { boxInfo } from "../macros";

export const DefineOptionsPlugin = definePlugin({
  name: "DefineOptions",
  enforce: "post",

  handled: false,
  pre() {
    this.handled = false;
  },

  transformDeclaration(item, s, ctx) {
    if (
      item.parent.type === "VariableDeclarator" &&
      item.parent.init?.type === "CallExpression"
    ) {
      if (item.parent.init.callee.type === "Identifier") {
        if (item.parent.init.callee.name !== "defineOptions") return;

        if (ctx.isSetup) {
          if (
            !item.parent.init.arguments ||
            item.parent.init.arguments.length === 0
          ) {
            return; // MacrosPlugin will handle validation
          }
          checkInheritAttrs(item.parent.init, ctx);
          ctx.items.push(createHelperImport(["defineOptions_Box"], ctx.prefix));

          const info = boxInfo("defineOptions", null, ctx);
          s.prependRight(item.declarator.start!, `let ${info.boxedName};`);
          s.prependLeft(
            item.parent.init.arguments[0].start!,
            `${info.boxedName}=${info.boxName}(`
          );
          s.prependRight(item.parent.init.arguments[0].end!, `)`);

          s.move(item.declarator.start!, item.parent.end, 0);
        } else {
          ctx.items.push({
            type: ProcessItemType.Warning,
            message: "INVALID_DEFINE_OPTIONS",
            node: item.node,
            // @ts-expect-error TODO improve this, this shouldn't be necessary
            start: item.node.start,
            // @ts-expect-error TODO improve this, this shouldn't be necessary
            end: item.node.end,
          });
        }
        this.handled = true;
      }
    }
  },
  transformFunctionCall(item, s, ctx) {
    if (item.name !== "defineOptions" || this.handled) {
      return;
    }

    if (ctx.isSetup) {
      if (!item.node.arguments || item.node.arguments.length === 0) {
        return; // MacrosPlugin will handle validation
      }
      ctx.items.push(createHelperImport(["defineOptions_Box"], ctx.prefix));
      checkInheritAttrs(item.node, ctx);

      const info = boxInfo("defineOptions", null, ctx);
      s.prependRight(item.node.start, `let ${info.boxedName};`);
      s.prependLeft(
        item.node.arguments[0].start!,
        `${info.boxedName}=${info.boxName}(`
      );
      s.prependRight(item.node.arguments[0].end!, `)`);
      s.move(item.node.start, item.node.end, 0);
    } else {
      ctx.items.push({
        type: ProcessItemType.Warning,
        message: "INVALID_DEFINE_OPTIONS",
        node: item.node,
        start: item.node.start,
        end: item.node.end,
      });
    }
  },
});

function checkInheritAttrs(node: CallExpression, ctx: ProcessContext) {
  const arg = node.arguments[0];
  if (!arg || arg.type !== "ObjectExpression") {
    return;
  }
  const inheritAttrsProp = arg.properties.find((x) => {
    return (
      x.type === "Property" &&
      x.key.type === "Identifier" &&
      x.key.name === "inheritAttrs"
    );
  });
  if (!inheritAttrsProp) {
    return;
  }
  if (inheritAttrsProp.type === "Property") {
    if (inheritAttrsProp.value.type === "Literal") {
      if (inheritAttrsProp.value.value === false) {
        ctx.items.push({
          type: ProcessItemType.InheritAttrs,
          node: inheritAttrsProp.value,
          value: false,
        });
      }
    }
  }
}
