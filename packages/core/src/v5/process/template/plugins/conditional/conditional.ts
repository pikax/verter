import { CommentNode, NodeTypes } from "@vue/compiler-core";
import {
  TemplateCondition,
  TemplateItemByType,
  TemplateTypes,
} from "../../../../parser/template/types";
import { declareTemplatePlugin } from "../../template";
import { ParseTemplateContext } from "../../../../parser/template";
import type * as babel_types from "@babel/types";
import { MagicString } from "@vue/compiler-sfc";

export const ConditionalPlugin = declareTemplatePlugin({
  name: "VerterConditional",

  // narrows: [] as {
  //   index: number;
  //   inBlock: boolean;
  //   conditions: TemplateCondition[];
  // }[],

  pre(_, ctx) {
    //   this.narrows.length = 0;

    ctx.toNarrow = [];

    ctx.doNarrow = (narrow: {
      index: number;
      inBlock: boolean;
      conditions: TemplateCondition[];

      type?: "prepend" | "append";
      direction?: "left" | "right";

      condition?: TemplateCondition | null;
    }, s: MagicString) => {
      const conditions = narrow.conditions.filter(
        (x) => x !== narrow.condition
      );
      if (conditions.length > 0) {
        const condition = narrow.inBlock
          ? generateBlockCondition(conditions, s)
          : generateTernaryCondition(conditions, s);

        if (narrow.type === "append") {
          if (narrow.direction === "right") {
            s.appendRight(narrow.index, condition);
          } else {
            s.appendLeft(narrow.index, condition);
          }
        } else {
          if (narrow.direction === "right") {
            s.prependRight(narrow.index, condition);
          } else {
            s.prependLeft(narrow.index, condition);
          }
        }
      }
    };
  },

  post(s, ctx) {
    if (!ctx.toNarrow) return;
    for (const narrow of ctx.toNarrow) {
      const conditions = narrow.conditions.filter(
        (x) => x !== narrow.condition
      );
      if (conditions.length > 0) {
        const condition = narrow.inBlock
          ? generateBlockCondition(conditions, s)
          : generateTernaryCondition(conditions, s);

        if (narrow.type === "append") {
          if (narrow.direction === "right") {
            s.appendRight(narrow.index, condition);
          } else {
            s.appendLeft(narrow.index, condition);
          }
        } else {
          if (narrow.direction === "right") {
            s.prependRight(narrow.index, condition);
          } else {
            s.prependLeft(narrow.index, condition);
          }
        }
      }
    }
  },

  transformCondition(item, s, ctx) {
    const element = item.element;
    const node = item.node;
    const rawName = node.rawName!;

    // slot render have special conditions and places where the v-if should be placed
    const canMove = !(
      element.tag === "template" && element.props.find((x) => x.name === "slot")
    );

    // Move comments to after the element contition narrow and
    // before the element condition
    // to respect top comments such as @ts-expect-any
    {
      const siblings = "children" in item.parent ? item.parent.children : [];
      if (siblings[0] !== element) {
        // if the element has previous siblings
        // we want to move the comments to after the narrow block
        const comments: CommentNode[] = [];
        for (let i = 0; i < siblings.length; i++) {
          const e = siblings[i];
          if (element === e) {
            break;
          }
          if (e.type === NodeTypes.COMMENT) {
            comments.push(e);
          } else {
            comments.length = 0;
          }
        }

        if (comments.length) {
          const from = Math.min(...comments.map((x) => x.loc.start.offset));
          s.move(from, element.loc.start.offset - 1, element.loc.start.offset);
        }
      }
    }

    if (canMove) {
      // move v-* to the beginning of the element
      s.move(
        node.loc.start.offset,
        node.loc.end.offset,
        element.loc.start.offset
      );
    }

    if (node.name === "else-if") {
      // replace '-' with ' '
      s.overwrite(node.loc.start.offset + 6, node.loc.start.offset + 7, " ");
    }

    // // remove v-
    s.remove(node.loc.start.offset, node.loc.start.offset + 2);

    // s.overwrite(
    //   node.loc.start.offset,
    //   node.loc.start.offset + rawName.length,
    //   "{"
    // );

    if (node.exp) {
      // this.conditions.set(
      //   node,
      //   s.slice(node.exp.loc.start.offset, node.exp.loc.end.offset).toString()
      // );

      // remove =
      s.remove(
        node.loc.start.offset + rawName.length,
        node.loc.start.offset + rawName.length + 1
      );

      // update delimiters
      s.overwrite(
        node.exp.loc.start.offset - 1,
        node.exp.loc.start.offset,
        "("
      );
      s.overwrite(node.exp.loc.end.offset, node.exp.loc.end.offset + 1, "){");
      // s.overwrite(node.exp.loc.end.offset, node.exp.loc.end.offset + 1, ")?");

      // remove delimiters
      //   s.remove(node.exp.loc.start.offset - 1, node.exp.loc.start.offset);
      //   s.remove(node.exp.loc.end.offset, node.exp.loc.end.offset + 1);

      //   s.overwrite(node.exp.loc.end.offset, node.exp.loc.end.offset + 1, "?");
    } else {
      //   s.prependLeft(node.loc.end.offset, ": undefined");

      // add {  after else
      s.prependLeft(node.loc.start.offset + rawName.length, "{");
    }
    s.prependLeft(element.loc.end.offset, "}");

    // narrow conditions
    if (ctx.narrow !== false) {
      if (item.context.conditions.length > 0) {
        const condition = generateBlockCondition(item.context.conditions, s);
        s.prependLeft(element.loc.start.offset, condition);
      }
    }
  },

  // transform(item, s, ctx) {
  //   if (item.type !== TemplateTypes.Function) {
  //     return;
  //   }
  //   console.log("item", item);

  //   const context = item.context as ParseTemplateContext;
  //   if (context.conditions.length === 0) {
  //     return;
  //   }

  //   const conditions = context.conditions;
  //   const node = item.node;

  //   conditions.map((x) =>
  //     s.slice(x.node.loc.start.offset, x.node.loc.end.offset).toString()
  //   );
  // },

  transformFunction(item, s, ctx) {
    if (
      ctx.narrow === undefined ||
      ctx.narrow === false ||
      (ctx.narrow !== true && !ctx.narrow.functions)
    ) {
      return;
    }

    const context = item.context as ParseTemplateContext;
    if (context.conditions.length === 0) {
      return;
    }
    const conditions = context.conditions;
    const node = item.node;

    const inBlock = node.type.indexOf("Arrow") === -1;
    s.prependRight(
      item.body.loc.start.offset,
      inBlock
        ? generateBlockCondition(conditions, s)
        : generateTernaryCondition(conditions, s)
    );
  },
});

function generateBlockCondition(
  conditions: TemplateCondition[],
  s: MagicString
) {
  const text = generateConditionText(conditions, s);
  return `if(!(${text})) return;`;
}

function generateTernaryCondition(
  conditions: TemplateCondition[],
  s: MagicString
) {
  const text = generateConditionText(conditions, s);
  return `!(${text})? undefined :`;
}

function generateConditionText(
  conditions: TemplateCondition[],
  s: MagicString
): string {
  const siblings = conditions
    .map((x) => x.siblings)
    .flat()
    .filter((x) => x);

  let negations = "";
  if (siblings.length > 0) {
    const st = generateConditionText(siblings, s);
    if (st) {
      negations = `!(${st})`;
    }
  }

  const positive = conditions
    .map((x) =>
      s.slice(x.node.loc.start.offset, x.node.loc.end.offset).toString()
    )
    .filter((x) => x)
    .map((x) => {
      const ending = x.endsWith("{") ? -1 : x.length;

      if (x.startsWith("if")) {
        x = x.slice(2, ending);
      } else if (x.startsWith("else if")) {
        x = x.slice(7, ending);
      } else if (x.startsWith("else")) {
        x = x.slice(4, ending);
      }
      return x;
    })
    .join(" && ");

  return [negations, positive].filter((x) => x).join(" && ");
}
