import { CommentNode, ElementNode, NodeTypes, Node } from "@vue/compiler-core";
import {
  TemplateCondition,
  TemplateElement,
  TemplateItemByType,
  TemplateTypes,
} from "../../../../parser/template/types";
import { declareTemplatePlugin } from "../../template";
import { ParseTemplateContext } from "../../../../parser/template";
import type * as babel_types from "@babel/types";
import { MagicString } from "@vue/compiler-sfc";
import { VerterASTNode } from "../../../../parser/ast";

export const ConditionalPlugin = declareTemplatePlugin({
  name: "VerterConditional",

  processed: new Set<TemplateElement | VerterASTNode | Node | ElementNode>(),

  // narrows: [] as {
  //   index: number;
  //   inBlock: boolean;
  //   conditions: TemplateCondition[];
  // }[],

  pre(_, ctx) {
    //   this.narrows.length = 0;
    this.processed.clear();

    ctx.doNarrow = (narrow, s: MagicString) => {
      if (narrow.parent) {
        if (this.processed.has(narrow.parent)) return;
        this.processed.add(narrow.parent);
      }
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

  // post(s, ctx) {
  //   if (!ctx.toNarrow) return;
  //   for (const narrow of ctx.toNarrow) {
  //     const conditions = narrow.conditions.filter(
  //       (x) => x !== narrow.condition
  //     );
  //     if (conditions.length > 0) {
  //       const condition = narrow.inBlock
  //         ? generateBlockCondition(conditions, s)
  //         : generateTernaryCondition(conditions, s);

  //       if (narrow.type === "append") {
  //         if (narrow.direction === "right") {
  //           s.appendRight(narrow.index, condition);
  //         } else {
  //           s.appendLeft(narrow.index, condition);
  //         }
  //       } else {
  //         if (narrow.direction === "right") {
  //           s.prependRight(narrow.index, condition);
  //         } else {
  //           s.prependLeft(narrow.index, condition);
  //         }
  //       }
  //     }
  //   }
  // },

  transformCondition(item, s, ctx) {
    const element = item.element;
    const node = item.node;
    const rawName = node.rawName!;
    this.processed.add(element);

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
    // s.prependRight(
    //   item.body.loc.start.offset,
    //   inBlock
    //     ? generateBlockCondition(conditions, s)
    //     : generateTernaryCondition(conditions, s)
    // );

    s.prependLeft(
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

/**
 * Generates a TypeScript condition expression for type narrowing in Vue templates.
 *
 * For v-if/v-else-if/v-else chains, this generates the combined condition that would be
 * true at that point in the chain. For example:
 * - v-if="A" → "A"
 * - v-else-if="B" (after v-if="A") → "!(A) && B"
 * - v-else-if="C" (after v-if="A" and v-else-if="B") → "!(A) && !(B) && C"
 * - v-else → "!(A) && !(B) && !(C)"
 *
 * The function processes siblings (prior conditions in the chain) by deduplicating them
 * to avoid the bug where nested sibling references would cause conditions to be repeated.
 *
 * @param conditions - The current condition(s) being processed (may include nested conditions)
 * @param s - MagicString containing the source code for extracting condition text
 * @returns A TypeScript expression that represents when this branch would be taken
 */
export function generateConditionText(
  conditions: TemplateCondition[],
  s: MagicString
): string {
  // Collect all unique siblings from all conditions (direct siblings only, no recursion)
  // Each condition's siblings are all prior conditions in the v-if/v-else-if chain
  const allSiblings = conditions
    .map((x) => x.siblings)
    .flat()
    .filter((x) => x);

  // Deduplicate siblings by node reference to avoid processing the same condition multiple times.
  // This fixes a bug where v-else-if chains would duplicate conditions because each sibling
  // also contains references to its own siblings (e.g., condition C has siblings [A, B],
  // and B has siblings [A], which would cause A to appear twice without deduplication).
  const uniqueSiblings = new Map<object, TemplateCondition>();
  for (const sibling of allSiblings) {
    if (!uniqueSiblings.has(sibling.node)) {
      uniqueSiblings.set(sibling.node, sibling);
    }
  }

  // Generate negation text for each sibling (without recursive sibling processing)
  const negationParts: string[] = [];
  for (const sibling of uniqueSiblings.values()) {
    const condText = s
      .slice(sibling.node.loc.start.offset, sibling.node.loc.end.offset)
      .toString();
    if (condText) {
      const parsed = parseConditionText(condText);
      if (parsed) {
        // Wrap compound conditions in parentheses before negating
        negationParts.push(`!(${wrapIfNeeded(parsed)})`);
      }
    }
  }

  const negations = negationParts.join(" && ");

  const positive = conditions
    .map((x) =>
      s.slice(x.node.loc.start.offset, x.node.loc.end.offset).toString()
    )
    .filter((x) => x)
    .map((x) => parseConditionText(x))
    .filter((x) => x)
    .map((x) => wrapIfNeeded(x))
    .join(" && ");

  return [negations, positive].filter((x) => x).join(" && ");
}

/**
 * Wraps a condition expression in parentheses if it contains operators that could
 * cause precedence issues when combined with && or !.
 *
 * @param expr - The condition expression
 * @returns The expression, wrapped in parentheses if needed
 */
function wrapIfNeeded(expr: string): string {
  // If already wrapped in parentheses, return as-is
  if (expr.startsWith("(") && expr.endsWith(")")) {
    // Check if the parentheses are balanced and wrap the entire expression
    let depth = 0;
    for (let i = 0; i < expr.length - 1; i++) {
      if (expr[i] === "(") depth++;
      else if (expr[i] === ")") depth--;
      if (depth === 0) {
        // The opening paren closes before the end, so it doesn't wrap everything
        return `(${expr})`;
      }
    }
    return expr;
  }

  // If contains && or || operators, wrap in parentheses
  // This ensures proper precedence when combined with outer && and !
  if (expr.includes("&&") || expr.includes("||")) {
    return `(${expr})`;
  }

  return expr;
}

/**
 * Parses a raw condition text from the source, removing prefixes like "if", "else if", "else"
 * and trailing braces.
 *
 * @param text - Raw condition text from the source (e.g., "if(isVisible){" or "else if(isNumber){")
 * @returns The extracted condition expression (e.g., "(isVisible)" or "(isNumber)")
 */
function parseConditionText(text: string): string {
  const ending = text.endsWith("{") ? -1 : text.length;

  if (text.startsWith("if")) {
    return text.slice(2, ending);
  } else if (text.startsWith("else if")) {
    return text.slice(7, ending);
  } else if (text.startsWith("else")) {
    return text.slice(4, ending);
  }
  return text.slice(0, ending);
}
