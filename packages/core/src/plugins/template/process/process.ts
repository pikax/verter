import { MagicString } from "@vue/compiler-sfc";
import { LocationType, WalkResult } from "../../types";
import { ParsedType, parse } from "../parse/parse";
import type { ParsedNodeBase } from "../parse/parse";
import {
  AttributeNode,
  ComponentNode,
  DirectiveNode,
  ElementNode,
  ElementTypes,
  ExpressionNode,
  ForParseResult,
  InterpolationNode,
  NodeTypes,
  PlainElementNode,
  SimpleExpressionNode,
  SourceLocation,
} from "@vue/compiler-core";
import { camelize, capitalize, isGloballyAllowed, makeMap } from "@vue/shared";
import type * as _babel_types from "@babel/types";
import { parse as acornParse } from "acorn-loose";

const isLiteralWhitelisted = /*#__PURE__*/ makeMap("true,false,null,this");

interface ProcessContext {
  // prevent the identifiers from getting the accessor prefixed
  // for example the v-for="item in items" has "item" added, because
  // is a blocked variable
  ignoredIdentifiers?: string[];

  // expose declarations, in case we need to import something
  // or declare a specific variable
  declarations?: WalkResult[];

  /**
   * Override the accessor for the template
   * @default "___VERTER__ctx"
   */
  accessor?: string;
  /**
   * Override the accessor for the template
   * @default "___VETER__comp"
   */
  componentAccessor?: string;

  slotAccessor?: string;

  conditions: {
    // current conditions
    ifs: string[];
    // other conditions
    elses: string[];
  };
}

export function process(
  parsed: ReturnType<typeof parse>,
  s: MagicString,
  overrideTemplate = false,
  context: ProcessContext = {
    ignoredIdentifiers: [],
    declarations: [],
    accessor: "___VERTER__ctx",
    componentAccessor: "___VERTER__comp",
    slotAccessor: "___VERTER_SLOT_COMP",
    conditions: {
      ifs: [],
      elses: [],
    },
  }
) {
  renderChildren(parsed.children, s, context);

  // clean <template> tag
  if (overrideTemplate && parsed.node.children.length > 0) {
    const firstChild = parsed.node.children[0];
    const lastChild = parsed.node.children[parsed.node.children.length - 1];
    s.overwrite(
      parsed.node.loc.start.offset,
      firstChild.loc.start.offset,
      "(\n<>\n"
    );

    s.overwrite(
      lastChild.loc.end.offset,
      parsed.node.loc.end.offset + parsed.node.source.length,
      "\n</>\n)"
    );
  }

  return {
    type: parsed.node.type,
    node: parsed.node,
    magicString: s,
    declarations: context.declarations,
  };
}

// function renderChildren(
//     children: ParsedNodeBase[],
//     s: MagicString,
//     context: ProcessContext
// ): void
function renderChildren(
  children: ParsedNodeBase[],
  s: MagicString,
  context: ProcessContext
): void {
  if (children?.length > 0) {
    for (let i = 0; i < children.length; i++) {
      const element = children[i];
      renderNode(element, s, context);
    }
  }
  // if (!children || children.length === 0) {

  // }
  // const c = children.map((x) => renderNode(x, ignoredIdentifiers));
  // return join ? c.join("\n") : c;
}

const render = {
  [ParsedType.PartialElement]: renderPartialElement,
  [ParsedType.Element]: renderElement,
  [ParsedType.Attribute]: renderAttribute,
  // [ParsedType.Directive]: renderDirective,
  // [ParsedType.Template]: renderTemplate,
  [ParsedType.Text]: renderText,
  [ParsedType.For]: renderFor,
  // [ParsedType.RenderSlot]: renderRenderSlot,
  [ParsedType.Slot]: renderSlot,
  [ParsedType.Comment]: renderComment,
  [ParsedType.Interpolation]: renderInterpolation,
  // [ParsedType.Condition]: renderCondition,
  [ParsedType.Condition]: renderConditionBetter,
};

function renderNode(
  node: ParsedNodeBase,
  s: MagicString,
  context: ProcessContext
) {
  try {
    return render[node.type](node, s, context);
  } catch (e) {
    console.error(node.type, e);
  }
}

function renderPartialElement(
  node: ParsedNodeBase & {
    node: ElementNode;
    tag?: string;
  },
  s: MagicString,
  context: ProcessContext
) {
  if (node.tag) {
    const tagIndex = node.content.indexOf(node.tag);
    appendCtx(node.tag, s, context, node.node.loc.start.offset + tagIndex);
  }
}

/**
 * Renders children in `$children` attribute, this is to allow strict type
 * checking
 * @param childrenPos position where $children it will be inserted
 * @param children
 * @param s
 * @param context
 */
function renderComponentChildren(
  childrenPos: number,
  children: ParsedNodeBase[],
  s: MagicString,
  context: ProcessContext
) {
  debugger;

  const conditionNarrow = generateNarrowCondition(context, false);
  const renderFunctionStart = `=>${conditionNarrow} <>\n`;
  const renderFunctionEnd = `\n</>}}`;

  // node in <my-el v-slot...>
  if (children.length === 1 && children[0].type === ParsedType.RenderSlot) {
    const child = children[0];
    const node = child.node as unknown as DirectiveNode;

    const name =
      retrieveStringExpressionNode(node.arg, undefined, context) ?? "default";

    // replace v-slot with $children
    const attributeStart = node.loc.start.offset;
    const attributeEnd = attributeStart + node.rawName.split(":")[0].length;

    s.overwrite(attributeStart, attributeEnd, "$children");

    const identifiers: string[] = [];

    let moveToIndex = attributeEnd;

    const { exp, arg } = node;
    if (exp && arg) {
      const expStart = exp.loc.start.offset;
      const expEnd = exp.loc.end.offset;

      // update delimiters
      s.overwrite(expStart - 1, expStart, "(");
      s.overwrite(expEnd, expEnd + 1, ")");

      // replace : with =
      s.overwrite(attributeEnd, attributeEnd + 1, "=");

      // append {{
      s.appendLeft(attributeEnd + 1, `{{`);

      // override = with :
      s.overwrite(
        attributeStart + node.rawName.length,
        attributeStart + node.rawName.length + 1,
        ":"
      );

      // s.appendLeft(expStart, `{{ ${name}: (`);
      s.appendLeft(expEnd + 1, renderFunctionStart);

      //if is an dynamic expression
      if (arg.loc.source[0] === "[") {
        retrieveStringExpressionNode(arg, s, context);
      }
    } else if (exp) {
      const expStart = exp.loc.start.offset;
      const expEnd = exp.loc.end.offset;

      // update delimiters
      s.remove(expStart - 1, expStart);
      s.remove(expEnd, expEnd + 1);

      s.appendLeft(expStart, `{{ ${name}: (`);
      s.appendLeft(expEnd, ")" + renderFunctionStart);

      moveToIndex = exp.loc.end.offset;
    } else if (arg) {
      // replace : with  ={{
      s.overwrite(attributeEnd, attributeEnd + 1, "={{\n");

      // no expression, append ()=>
      s.appendLeft(
        attributeStart + node.rawName.length,
        ": ()" + renderFunctionStart
      );

      //if is an dynamic expression
      if (arg.loc.source[0] === "[") {
        retrieveStringExpressionNode(arg, s, context);
      }

      moveToIndex = arg.loc.end.offset;
    } else {
      // no expression add ={
      s.appendLeft(attributeEnd, "={{");

      // no name
      // replace v-slot with default: ()=>
      s.appendLeft(attributeEnd, `${name}: ()${renderFunctionStart}`);
    }

    if (exp) {
      if (exp.ast) {
        // AST is actually an arrow functionsa
        for (const param of exp.ast.params) {
          identifiers.push(...retrieveAccessors(param, context));
        }
      } else {
        identifiers.push(exp.loc.source);
      }
    }

    try {
      // move children
      for (const it of child.children) {
        s.move(
          it.node.loc.start.offset,
          it.node.loc.end.offset,
          // set it after expression or after the attribute end
          // node.exp?.loc.end.offset ?? node.arg?.loc.end.offset ?? attributeEnd
          moveToIndex
        );
      }

      renderChildren(child.children, s, {
        ...context,
        ignoredIdentifiers: [...context.ignoredIdentifiers, ...identifiers],
      });
    } catch (e) {
      console.error(e);
    }

    if (node.exp) {
      s.appendRight(
        node.exp.loc.end.offset + (node.arg ? 1 : 0),
        renderFunctionEnd
      );
    } else if (node.arg) {
      s.appendRight(node.arg.loc.end.offset, renderFunctionEnd);
    } else {
      // no expression add }}
      s.appendRight(attributeEnd, renderFunctionEnd);
    }
  }
}

function renderSlot(
  node: ParsedNodeBase & {
    tag: "slot";
    props: ParsedNodeBase[];
  },
  s: MagicString,
  context: ProcessContext
) {
  // this renders slots <slot>

  // TODO add information in the context

  const startIndex = node.node.loc.start.offset;
  const endIndex = node.node.loc.end.offset;

  const tagNameEndIndex = startIndex + 1 + node.tag.length;
  const firstAttributeIndex = node.props.length
    ? Math.min(...node.props.map((x) => x.node.loc.start.offset - 1))
    : tagNameEndIndex;

  // replace < with {()=>{\n
  s.overwrite(startIndex, startIndex + 1, `${node.NO_WRAP ? "" : "{"}()=>{\n`, {
    contentOnly: true,
  });

  if ("isSelfClosing" in node.node && node.node.isSelfClosing) {
    // replace last `/>` with \n}}
    // s.prependLeft(endIndex, "\n}}");
    s.prependLeft(endIndex, `\n${node.NO_WRAP ? "" : "}"}}`);
  } else {
    const lastKnownStartIndex = Math.max(
      ...(node.children.length > 0
        ? // if there's children get the last offset of the children
          node.children!.map((x) => x.node.loc.end.offset)
        : // otherwise
          node.props.map((x) => x.node.loc.end.offset + 1))
    );
    const slotNameIndex = s.original.indexOf("slot", lastKnownStartIndex);

    s.overwrite(slotNameIndex, slotNameIndex + "slot".length, "Comp");
    // s.prependLeft(endIndex, "\n}}");
    s.prependLeft(endIndex, `\n${node.NO_WRAP ? "" : "}"}}`);
  }

  const nameAttribute = node.props.find(
    (x) =>
      (retrieveStringExpressionNode(x.node.arg, undefined, context) ??
        x.node.name) === "name"
  );
  // move name attribute to the beginning
  if (nameAttribute) {
    // only move to the first attribute, because if there's an v-if we need to be first
    // attribute

    if (nameAttribute.node.loc.start.offset > firstAttributeIndex + 1) {
      // don't append really at the end because it might cause issues when overriding it
      s.move(
        nameAttribute.node.loc.start.offset,
        nameAttribute.node.loc.end.offset,
        firstAttributeIndex + 1
      );
    }
  }

  // replace slot with ___VERTER_SLOT_COMP
  s.overwrite(
    startIndex + 1,
    tagNameEndIndex,
    context.slotAccessor + `${nameAttribute ? "" : ".default"}`,
    {
      contentOnly: true,
    }
  );

  // prepending variable `declaration const Comp = `
  s.prependRight(startIndex + 1, "const Comp = ");

  // check if the nameAttribute was moved, if it was we prependRight
  if (
    !nameAttribute ||
    nameAttribute.node.loc.start.offset > firstAttributeIndex + 1
  ) {
    s.prependRight(
      firstAttributeIndex +
        (nameAttribute?.node.type === NodeTypes.DIRECTIVE ||
        !node.node.isSelfClosing
          ? 1
          : 0),
      "\nreturn <Comp "
    );
  } else {
    s.prependRight(nameAttribute.node.loc.end.offset, "\nreturn <Comp ");
  }

  // process non-name attributes

  renderAttributes(
    node.props.filter((x) => x !== nameAttribute),
    s,
    context
  );
  // replace name attribute
  if (nameAttribute) {
    if (nameAttribute.node.type === NodeTypes.ATTRIBUTE) {
      s.overwrite(
        nameAttribute.node.nameLoc.start.offset,
        // add +1 to remove = + '
        nameAttribute.node.nameLoc.end.offset + 2,
        "."
      );

      s.remove(
        nameAttribute.node.value.loc.end.offset - 1,
        nameAttribute.node.value.loc.end.offset
      );
    } else if (nameAttribute.node.type === NodeTypes.DIRECTIVE) {
      if ("exp" in nameAttribute.node && nameAttribute.node.exp) {
        retrieveStringExpressionNode(nameAttribute.node.exp, s, context);

        // replace :name with [
        s.overwrite(
          nameAttribute.node.loc.start.offset,
          nameAttribute.node.exp.loc.start.offset,
          "["
        );
        // replace delimetor with ]
        s.overwrite(
          nameAttribute.node.exp.loc.end.offset,
          nameAttribute.node.exp.loc.end.offset + 1,
          "]"
        );
      } else {
        appendCtx(nameAttribute.node.arg, s, context);

        // replace `:` with `[`
        s.overwrite(
          nameAttribute.node.loc.start.offset,
          nameAttribute.node.loc.start.offset + 1,
          "["
        );

        // add `]`
        s.appendLeft(nameAttribute.node.loc.end.offset, "]");
      }
    }
  }

  renderChildren(node.children, s, context);
}

function renderElement(
  node: ParsedNodeBase & {
    node: ElementNode;
    props: ParsedNodeBase[];
    tag: string;
  },
  s: MagicString,
  context: ProcessContext
) {
  // const content = node.props.map((x) => renderNode(x, s, ignoredIdentifiers, declarations)).join("\n");
  const { name: tag, accessor } = resolveComponentTag(node.node, context);

  const openTagIndex = node.node.loc.start.offset;
  const closeTagIndex =
    node.node.loc.source.lastIndexOf(node.tag) + openTagIndex;
  // NOTE this override is quite big, it does not provide a 1-1 mapping per character
  if (tag !== node.tag) {
    s.overwrite(openTagIndex + 1, openTagIndex + node.tag.length + 1, tag);
    if (closeTagIndex > openTagIndex + node.tag.length) {
      s.overwrite(closeTagIndex, closeTagIndex + node.tag.length, tag);
    }
  }
  if (accessor) {
    s.prependLeft(openTagIndex + 1, `${accessor}.`);
    if (closeTagIndex > openTagIndex + node.tag.length) {
      s.prependLeft(closeTagIndex, `${accessor}.`);
    }
  }

  renderAttributes(node.props, s, context);

  // renderChildren(node.props, s, ignoredIdentifiers, declarations);
  // s.appendRight(node., closeTag);
  // s.overwrite(node.node.loc.end.offset, node.node.loc.end.offset + closeTag.length, closeTag);
  if (!node.node.isSelfClosing) {
    const closeTagIndex = node.node.loc.end.offset;
    s.overwrite(closeTagIndex - node.tag.length - 1, closeTagIndex - 1, tag);
  }

  if (node.node.tagType === ElementTypes.COMPONENT) {
    const lastKnownIndex =
      node.children.find((x) => x.type === ParsedType.RenderSlot)?.node.loc
        .start.offset ??
      (node.props.length
        ? Math.max(...node.props.map((x) => x.node.loc.end.offset))
        : openTagIndex + node.node.tag.length + 1);

    renderComponentChildren(lastKnownIndex, node.children, s, context);
  } else {
    renderChildren(node.children, s, context);
  }
}

function renderAttributes(
  attributes: ParsedNodeBase[],
  s: MagicString,
  context: ProcessContext
) {
  const classesToNormalise: ParsedNodeBase[] = [];
  const stylesToNormalise: ParsedNodeBase[] = [];
  for (let i = 0; i < attributes.length; i++) {
    const attribute = attributes[i];

    // const name =
    //   attribute.node.name ?? retrieveStringExpressionNode(attribute.node.arg);

    let name =
      retrieveStringExpressionNode(attribute.node.arg, undefined, context) ??
      attribute.node.name;

    if (name === "class") {
      classesToNormalise.push(attribute);
    } else if (name === "style") {
      stylesToNormalise.push(attribute);
    } else {
      renderAttribute(attribute, s, context);
    }
  }

  if (classesToNormalise.length > 1) {
    // find the first binding attribute, the other ones
    // will be moved in
    const firstBindClass = classesToNormalise.find(
      (x) => x.directive === "bind"
    );
    renderAttribute(firstBindClass, s, context);

    // append at the end of firstBindClass
    const startIndex = (firstBindClass.expression ?? firstBindClass.exp).loc
      .start.offset;
    const endIndex = (firstBindClass.expression ?? firstBindClass.exp).loc.end
      .offset;

    s.prependRight(startIndex, "__VERTER__normalizeClass([");
    s.prependRight(endIndex, "])");

    try {
      for (const attrClass of classesToNormalise) {
        if (attrClass === firstBindClass) {
          continue;
        }

        const node = attrClass.node;

        let loc: SourceLocation | null = null;

        if ("value" in node) {
          loc = node.value.loc;
        } else {
          loc = (node.exp ?? node.expression).loc;
        }

        if (loc) {
          // tried to append, but the moving was breaking things
          s.overwrite(
            loc.start.offset,
            loc.start.offset + 1,
            `,${loc.source[0]}`
          );
          s.move(loc.start.offset, loc.end.offset, endIndex);

          s.remove(node.loc.start.offset, loc.start.offset);
          s.remove(loc.end.offset, node.loc.end.offset);
        } else {
          console.error("Unknown error happened!!!");
        }
        if (node.exp) {
          retrieveStringExpressionNode(node.exp, s, context, true);
        }
      }

      // classesToNormalise.forEach((x) => renderAttribute(x, s, context));
    } catch (e) {
      console.error(e);
    }
  } else {
    // if just one, just do the normal mapping
    classesToNormalise.forEach((x) => renderAttribute(x, s, context));
  }

  if (stylesToNormalise.length > 1) {
    // find the first binding attribute, the other ones
    // will be moved in
    const firstBindStyle = stylesToNormalise.find(
      (x) => x.directive === "bind"
    );
    // renderAttribute(firstClass, s, context);
    renderAttribute(firstBindStyle, s, context);

    // append at the end of firstBindClass
    const startIndex = (firstBindStyle.expression ?? firstBindStyle.exp).loc
      .start.offset;
    const endIndex = (firstBindStyle.expression ?? firstBindStyle.exp).loc.end
      .offset;

    s.prependRight(startIndex, "__VERTER__normalizeStyle([");
    s.prependRight(endIndex, "])");

    try {
      for (const attrStyle of stylesToNormalise) {
        if (attrStyle === firstBindStyle) {
          continue;
        }

        const node = attrStyle.node;

        let loc: SourceLocation | null = null;

        if ("value" in node) {
          loc = node.value.loc;
        } else {
          loc = (node.exp ?? node.expression).loc;
        }

        if (loc) {
          // tried to append, but the moving was breaking things
          s.overwrite(
            loc.start.offset,
            loc.start.offset + 1,
            `,${loc.source[0]}`
          );
          s.move(loc.start.offset, loc.end.offset, endIndex);

          s.remove(node.loc.start.offset, loc.start.offset);
          s.remove(loc.end.offset, node.loc.end.offset);
        } else {
          console.error("Unknown error happened!!!");
        }

        if (node.exp) {
          retrieveStringExpressionNode(node.exp, s, context, true);
        }
      }
    } catch (e) {
      console.error(e);
    }
  } else {
    // if just one, just do the normal mapping
    stylesToNormalise.forEach((x) => renderAttribute(x, s, context));
  }

  if (stylesToNormalise.length > 1 || classesToNormalise.length > 1) {
    context.declarations.push({
      type: LocationType.Import,
      from: "vue",
      generated: true,
      node: undefined,
      items: [
        classesToNormalise.length > 1
          ? {
              name: "normalizeClass",
              alias: "__VERTER__normalizeClass",
            }
          : undefined,
        stylesToNormalise.length > 1
          ? {
              name: "normalizeStyle",
              alias: "__VERTER__normalizeStyle",
            }
          : undefined,
      ].filter(Boolean),
    });
  }
}

const ATTRIBUTE_NAME_WHITELIST = ["aria-", "data-"];
function sanitiseAttributeName(name: string) {
  if (ATTRIBUTE_NAME_WHITELIST.some((x) => name.startsWith(x))) {
    return name;
  }
  return camelize(name);
}

function renderAttribute(
  node: ParsedNodeBase,
  s: MagicString,
  context: ProcessContext
) {
  const n = node.node as unknown as AttributeNode | DirectiveNode;

  // if there's an attribute only camialize if not an element
  if ((n as AttributeNode).nameLoc && node.parent?.tagType !== 0) {
    const cameled = sanitiseAttributeName(n.name);
    if (cameled !== n.name) {
      s.overwrite(n.nameLoc.start.offset, n.nameLoc.end.offset, cameled);
    }
  }

  if (n.type === NodeTypes.ATTRIBUTE) {
  } else if (n.type === NodeTypes.DIRECTIVE) {
    if (n.name === "bind") {
      // const name = n.arg;
      const name = retrieveStringExpressionNode(n.arg, undefined, context);
      // if is content let it apply the mapping
      //   const content = retriveStringExpressionNode(n.exp, s, context);

      const sanitisedName = name ? sanitiseAttributeName(name) : name;

      if (sanitisedName !== name) {
        s.overwrite(
          n.arg.loc.start.offset,
          n.arg.loc.end.offset,
          sanitisedName
        );
      }

      if (n.exp) {
        retrieveStringExpressionNode(
          n.exp,
          s,
          context,
          true,
          -1, // - (n.rawName[0] === ":" ? 1 : 0),
          true
        );
      }

      if (name !== n.rawName) {
        // used to replace \" or \' with \{ or \}
        let updateDelimiter = false;

        switch (n.rawName[0]) {
          case ":": {
            if (n.exp) {
              s.overwrite(n.loc.start.offset, n.loc.start.offset + 1, "");
              updateDelimiter = true;
            } else {
              // I would like to do this, maping the whoel :name to name={name}
              //   s.overwrite(
              //     n.loc.start.offset,
              //     n.loc.end.offset,
              //     `${name}={${appendCtx(name, undefined, context)}}`
              //   );
              // but probably is better to do something else for renaming :/
              // short binding :name -> name={name}
              //   s.overwrite(n.loc.start.offset, n.loc.start.offset + 1, "");
              //   s.move(
              //     n.loc.start.offset,
              //     n.loc.start.offset + 1,
              //     n.loc.end.offset + 1
              //   );

              // this provides a better mapping, making `:` to be =_ctx.{name}
              s.overwrite(
                n.loc.start.offset,
                n.loc.start.offset + 1,
                `={${appendCtx(sanitisedName, undefined, context)}}`
              );
              s.move(
                n.loc.start.offset,
                n.loc.start.offset + 1,
                n.loc.end.offset
              );
            }
            break;
          }
          case "@": {
            s.overwrite(n.loc.start.offset, n.loc.start.offset + 1, "on");
            updateDelimiter = true;
            break;
          }
          // v-bind:
          case "v": {
            if (name) {
              s.remove(
                n.loc.start.offset,
                n.loc.start.offset + "v-bind:".length
              );
            } else {
              // move v-bind:[*]\" delimiter to before the v-bind;
              const start = n.exp.loc.start.offset - 1;
              s.move(start, start + 1, n.loc.start.offset);
              //   s.move();

              // v-bind="var" -> {...var}
              s.overwrite(
                n.loc.start.offset,
                n.loc.start.offset + "v-bind=".length,
                "..."
              );
            }
            updateDelimiter = true;

            // TODO handle v-on, v-bind, v-model || not sure if those are meant to be here, since the n.name should be on or model
            break;
          }
          // TODO handle modifiers
        }

        if (updateDelimiter) {
          const start = n.exp.loc.start.offset - 1;
          s.overwrite(start, start + 1, "{", { contentOnly: true }); // replace : with {
          //   s.move(start, n.exp.loc.start.offset - 1, n.loc.start.offset + 1); // replace : with {
          const end = n.exp.loc.end.offset;
          s.overwrite(end, end + 1, "}", { contentOnly: true }); // replace : with }
        }
      }

      // if(n.rawName[0])
    } else if (n.name === "on") {
      const name = retrieveStringExpressionNode(n.arg, undefined, context);

      const fixedName = capitalize(camelize(name));
      if (fixedName !== name) {
        s.overwrite(n.arg.loc.start.offset, n.arg.loc.end.offset, fixedName);
      }

      if (n.exp) {
        // TODO wrap expression in ()=>  + conditions
        retrieveStringExpressionNode(
          n.exp,
          s,
          context,
          true
          // n.exp.loc.start.offset
        );
      }

      if (name !== n.rawName) {
        let updateDelimiter = false;

        switch (n.rawName[0]) {
          case "@": {
            s.overwrite(n.loc.start.offset, n.loc.start.offset + 1, "on");
            updateDelimiter = true;
            break;
          }
        }
        if (updateDelimiter) {
          const start = n.exp.loc.start.offset - 1;
          s.overwrite(start, start + 1, "{", { contentOnly: true }); // replace : with {
          //   s.move(start, n.exp.loc.start.offset - 1, n.loc.start.offset + 1); // replace : with {
          const end = n.exp.loc.end.offset;
          s.overwrite(end, end + 1, "}", { contentOnly: true }); // replace : with }
        }
      }
    } else {
      // TODO
    }
  }

  //   const name = retriveStringExpressionNode(n.arg, ignoredIdentifiers) || n.name;
  //   const content =
  //     retriveStringExpressionNode(n.exp, ignoredIdentifiers) ||
  //     n.value?.content ||
  //     n.value;

  //   if (n.name === "bind") {
  //     if (!!n.arg) {
  //       return `${name}={${content ?? name}}`;7
  //     }
  //     return `{...${content}}`;
  //   }
  //   if (n.name === "on") {
  //     return `on${capitalize(name)}={${content}}`;
  //   }
  //   if (n.type === NodeTypes.DIRECTIVE) {
  //     return `${name}={${content}}`;
  //   }

  //   return `${name}={${JSON.stringify(content)}}`;
}

function renderText(
  node: ParsedNodeBase,
  s: MagicString,
  context: ProcessContext
) {
  // if has content besides whitespace
  if (node.content.trim()) {
    s.overwrite(
      node.node.loc.start.offset,
      node.node.loc.end.offset,
      `{ ${JSON.stringify(node.content)} }`
    );
  }
}

function renderFor(
  node: ParsedNodeBase & {
    for: ForParseResult;
    expression: SimpleExpressionNode;
  },
  s: MagicString,
  context: ProcessContext
) {
  const { source, value, key, index } = node.for;

  const keyString = key
    ? retrieveStringExpressionNode(key, undefined, context, false)
    : "";

  const sourceString = source
    ? retrieveStringExpressionNode(source, s, context)
    : "()";

  const valueString = value
    ? retrieveStringExpressionNode(value, undefined, context, false)
    : "value";

  const ignoredIdentifiers = [
    ...context.ignoredIdentifiers,
    valueString,
    keyString,
  ].filter(Boolean);

  const indexString = index
    ? retrieveStringExpressionNode(index, undefined, {
        ...context,
        ignoredIdentifiers,
      })
    : "";

  // TODO content
  //   const content = renderChildren(
  //     node.children,
  //     [...ignoredIdentifiers, valueString, keyString, indexString].filter(Boolean)
  //   );

  const childStart = node.children[0].node.loc.start.offset;
  const childEnd = node.children[node.children.length - 1].node.loc.end.offset;
  const vForStart = node.node.loc.start.offset;
  const vForEnd = node.node.loc.end.offset;

  const shouldWrapParentesis = !node.expression.content.startsWith("(");

  // index of in or of, can be " in " or " of "

  const InOf = [
    " in ",
    "   in ",
    "   in   ",
    " in    ",

    " of ",
    "   of ",
    "   of   ",
    " of    ",
  ];

  let inOfIndex = -1;
  for (let i = 0; i < InOf.length && inOfIndex === -1; i++) {
    inOfIndex = node.expression.content.indexOf(InOf[i]);
  }
  // couldn't find 'in' or 'of'
  if (inOfIndex < 0) {
    throw new Error("Invalid v-for expression");
  }

  const startInOf = node.expression.loc.start.offset + inOfIndex;
  const endInOf = startInOf + 4;

  // move " in " or " of " after source
  // <li v-for="item in items"> -> <li v-for="itemitems in ">
  s.move(
    startInOf,
    endInOf,
    source.loc.end.offset < endInOf ? endInOf + 1 : source.loc.end.offset
  );

  // replace in / of with a comma
  // <li v-for="item in items"> -> <li v-for="itemitems,">
  s.overwrite(startInOf, endInOf, "," + (shouldWrapParentesis ? "(" : ""));

  // <li v-for="itemitems,"> -> v-for="itemitems,"<li >
  s.move(vForStart, vForEnd, childStart);

  // add {  }
  // v-for="itemitems,"<li > -> {v-for="itemitems,"<li >}
  if (!node.NO_WRAP) {
    s.prependLeft(childStart, "{");
    s.appendRight(childEnd, "}");
  }

  // {v-for="itemitems,"<li >} -> { renderList"itemitems,"<li
  s.overwrite(vForStart, vForStart + "v-for=".length, `__VERTER__renderList`);

  context.declarations.push({
    type: LocationType.Import,
    generated: true,
    from: "vue",
    node: null,
    items: [
      {
        name: "renderList",
        alias: "__VERTER__renderList",
      },
    ],
  });

  // { renderList"item in _ctx.items"<li -> { renderList(item in _ctx.items)<li
  //   if (value) {
  s.overwrite(
    node.expression.loc.start.offset - 1,
    node.expression.loc.start.offset,
    `(`
  );
  //   } else {
  //     // TODO
  //   }
  if (source) {
    // add `)` if we should wrap
    const relacer = `${shouldWrapParentesis ? ")" : ""}=>{`;
    s.overwrite(source.loc.end.offset, source.loc.end.offset + 1, relacer);
  } else {
    // TODO
  }

  // { renderList(item in _ctx.items)<li -> { renderList(_ctx.items, (item)<li
  if (value) {
    // move the value to after the source
    s.move(
      node.expression.loc.start.offset,
      node.expression.loc.start.offset + inOfIndex,
      source.loc.end.offset
    );
  } else {
    // TODO
  }

  // close v-for
  s.appendLeft(childEnd, "})");

  // append conditions
  const narrowConditions = generateNarrowCondition(context, !node.NO_WRAP);
  if (narrowConditions) {
    s.prependRight(childStart, narrowConditions);
  }

  const childrenContext = {
    ...context,
    ignoredIdentifiers,
  };

  renderChildren(node.children, s, childrenContext);
}

function renderConditionNarrowed(
  node: ParsedNodeBase & { conditions: ParsedNodeBase[] },
  s: MagicString,
  context: ProcessContext
) {
  const { conditions } = node;

  const firstCondition = conditions[0];
  const lastCondition = conditions[conditions.length - 1];

  const expressionAccessors: Array<string> = [];

  if (firstCondition) {
    const firstChild =
      firstCondition.children[0]?.type === "for"
        ? firstCondition.children[0]?.children[0]
        : firstCondition.children[0];
    const childStart = firstChild.node.loc.start.offset;

    // append { ()=> {

    s.prependLeft(childStart, "{ ()=> {");

    const lastChild =
      lastCondition.children[lastCondition.children.length - 1]?.type === "for"
        ? lastCondition.children[lastCondition.children.length - 1]?.children[
            lastCondition.children[lastCondition.children.length - 1]?.children
              .length - 1
          ]
        : lastCondition.children[lastCondition.children.length - 1];
    const childEnd = lastChild.node.loc.end.offset;
    s.appendRight(childEnd, " } }");
  }

  for (let i = 0; i < conditions.length; i++) {
    const condition = conditions[i];

    const firstChild =
      condition.children[0]?.type === "for"
        ? condition.children[0]?.children[0]
        : condition.children[0];
    const lastChild =
      condition.children[condition.children.length - 1]?.type === "for"
        ? condition.children[condition.children.length - 1]?.children[
            condition.children[condition.children.length - 1]?.children.length -
              1
          ]
        : condition.children[condition.children.length - 1];

    const childStart = firstChild.node.loc.start.offset;
    const childEnd = lastChild.node.loc.end.offset;
    const conditionStart = condition.node.loc.start.offset;
    const conditionEnd = condition.node.loc.end.offset;

    const directiveStart = condition.node.loc.start.offset;
    const directiveEnd = directiveStart + condition.node.rawName.length + 1; // with =

    if (condition.directive !== "else") {
      // move v-if/v-else-if to before the first child
      s.move(conditionStart - 1, conditionEnd, childStart);
    } /*else {
      // move v-if/v-else-if to before the first child
      s.move(conditionStart, conditionEnd, childStart + 1);
    }*/

    switch (condition.directive) {
      case "if": {
        // move v-if to after the expression

        //remove v-
        s.remove(directiveStart, directiveStart + 2);

        // remove =
        s.remove(directiveEnd - 1, directiveEnd);

        // // replace " " with ( )
        // s.overwrite(directiveEnd, directiveEnd + 1, "(");
        // s.overwrite(
        //   condition.expression.loc.end.offset,
        //   condition.expression.loc.end.offset + 1,
        //   ")"
        // );

        // // now the expression is at childStart
        // s.move(directiveStart, directiveEnd, childStart);

        // // replace v-if with ?
        // s.overwrite(directiveStart, directiveEnd, "?");

        // add accessors

        // replace v-if with
        break;
      }
      case "else-if": {
        // now the expression is at childStart
        s.move(directiveStart, directiveEnd, childStart);

        // replace v-else-if with ?
        s.overwrite(directiveStart, directiveEnd, "?");
        break;
      }
      case "else": {
        // // now the expression is at childStart
        // directive end contains `=`
        s.move(directiveStart, directiveEnd - 1, childStart);

        // replave v-else with :
        s.overwrite(directiveStart, directiveEnd - 1, " else ");
        // s.move(directiveStart, directiveStart + 1, childStart);

        break;
      }
    }

    // add wrap { } to children

    if ("expression" in condition && condition.expression) {
      const expression = condition.expression as ExpressionNode;

      if (expression.ast) {
        expressionAccessors.push(...retrieveAccessors(expression.ast, context));
      }

      // replace delimiters with ()
      s.overwrite(
        expression.loc.start.offset - 1,
        expression.loc.start.offset,
        // (condition.directive === "else-if" ? "}" : "") + "("
        "("
      );
      s.overwrite(
        expression.loc.end.offset,
        expression.loc.end.offset + 1,
        ")"
      );

      retrieveStringExpressionNode(expression, s, context, true);
    }
    // append new accessors
    let overriddenContext = context;
    if (expressionAccessors.length) {
      const prevAccessor = context.accessor + ".";
      const accessor = context.accessor + "_narrower";

      const newAccessorStr = `\nconst ${accessor} = {...${
        context.accessor
      }, ${expressionAccessors.map(
        (x) => `${x.replace(prevAccessor, "")}: ${x}`
      )}};\n`;
      // s.prependLeft(expression.loc.end.offset + 1, newAccessorStr);
      s.prependRight(childStart, newAccessorStr);

      overriddenContext = {
        ...context,
        accessor,
      };
    }

    renderChildren(
      condition.children.map((x) => {
        if (x.type === "for" || x.type === "slot") {
          // prevent wrapping of the for loop
          x.NO_WRAP = true;
        }
        return x;
      }),
      s,
      context
    );

    if (condition.children.length > 0) {
      // add {  }
      s.prependRight(childStart, "{");
      s.prependLeft(childEnd, "}");
    }
  }
}

function renderConditionBetter(
  node: ParsedNodeBase & { conditions: ParsedNodeBase[] },
  s: MagicString,
  context: ProcessContext
) {
  const { conditions } = node;

  let hasElse = false;

  const firstCondition = conditions[0];
  const lastCondition = conditions[conditions.length - 1];

  if (firstCondition) {
    const firstChild =
      firstCondition.children[0]?.type === "for"
        ? firstCondition.children[0]?.children[0]
        : firstCondition.children[0];
    const childStart = firstChild.node.loc.start.offset;
    s.prependLeft(childStart, "{");

    const lastChild =
      lastCondition.children[lastCondition.children.length - 1]?.type === "for"
        ? lastCondition.children[lastCondition.children.length - 1]?.children[
            lastCondition.children[lastCondition.children.length - 1]?.children
              .length - 1
          ]
        : lastCondition.children[lastCondition.children.length - 1];
    const childEnd = lastChild.node.loc.end.offset;
    s.appendRight(childEnd, "}");
  }

  const conditionsContent: string[] = [];

  for (let i = 0; i < conditions.length; i++) {
    const condition = conditions[i];

    const firstChild =
      condition.children[0]?.type === "for"
        ? condition.children[0]?.children[0]
        : condition.children[0];
    const lastChild =
      condition.children[condition.children.length - 1]?.type === "for"
        ? condition.children[condition.children.length - 1]?.children[
            condition.children[condition.children.length - 1]?.children.length -
              1
          ]
        : condition.children[condition.children.length - 1];

    const childStart = firstChild.node.loc.start.offset;
    const childEnd = lastChild.node.loc.end.offset;
    const conditionStart = condition.node.loc.start.offset;
    const conditionEnd = condition.node.loc.end.offset;

    const directiveStart = condition.node.loc.start.offset;
    const directiveEnd = directiveStart + condition.node.rawName.length + 1; // with =

    if (condition.directive !== "else") {
      // move v-if/v-else-if to before the first child
      s.move(conditionStart, conditionEnd, childStart);

      // add {  }
      //   s.appendLeft(childStart, "{");
      //   s.appendLeft(childEnd, "}");
    } /*else {
      // move v-if/v-else-if to before the first child
      s.move(conditionStart, conditionEnd, childStart + 1);
    }*/

    switch (condition.directive) {
      case "if": {
        // move v-if to after the expression

        // now the expression is at childStart
        s.move(directiveStart, directiveEnd, childStart);

        // replace v-if with ?
        s.overwrite(directiveStart, directiveEnd, "?");
        break;
      }
      case "else-if": {
        // now the expression is at childStart
        s.move(directiveStart, directiveEnd, childStart);

        // replace v-else-if with ?
        s.overwrite(directiveStart, directiveEnd, "?");
        break;
      }
      case "else": {
        // // now the expression is at childStart
        // directive end contains `=`
        s.move(directiveStart, directiveEnd - 1, childStart);

        // replave v-else with :
        s.overwrite(directiveStart, directiveEnd - 1, ":");
        // s.move(directiveStart, directiveStart + 1, childStart);
        hasElse = true;

        break;
      }
    }

    if ("expression" in condition && condition.expression) {
      const expression = condition.expression as ExpressionNode;

      // replace delimiters with ()
      s.overwrite(
        expression.loc.start.offset - 1,
        expression.loc.start.offset,
        (condition.directive === "else-if" ? ":" : "") + "("
      );
      s.overwrite(
        expression.loc.end.offset,
        expression.loc.end.offset + 1,
        ")"
      );

      retrieveStringExpressionNode(expression, s, context, true);
    }

    // s.overwrite(conditionStart - 1, conditionStart)

    // renderConditionNode(condition, s, context);

    // for (let i = 0; i < condition.children.length; i++) {
    //   const child = condition.children[i];
    //   renderNode(child, s, context);
    // }

    let currentCondition = condition.expression
      ? s.snip(conditionStart, conditionEnd).toString()
      : "";
    // there's an ? on the last because of moving
    if (currentCondition.endsWith("?")) {
      currentCondition = currentCondition.slice(0, -1);
    }

    renderChildren(
      condition.children.map((x) => {
        if (x.type === "for" || x.type === "slot") {
          // prevent wrapping of the for loop
          x.NO_WRAP = true;
        }
        return x;
      }),
      s,
      {
        ...context,
        conditions: {
          ifs: [...context.conditions.ifs, currentCondition].filter(Boolean),
          elses: [...context.conditions.elses, ...conditionsContent],
        },
      }
    );

    if (currentCondition) {
      conditionsContent.push(currentCondition);
    }
  }

  if (!hasElse) {
    const lastCondition = conditions[conditions.length - 1];

    const lastChild =
      lastCondition.children[lastCondition.children.length - 1]?.type === "for"
        ? lastCondition.children[lastCondition.children.length - 1]?.children[
            lastCondition.children[lastCondition.children.length - 1]?.children
              .length - 1
          ]
        : lastCondition.children[lastCondition.children.length - 1];
    const childEnd = lastChild.node.loc.end.offset;
    s.prependRight(childEnd, " : undefined");
  }
}

function renderCondition(
  node: ParsedNodeBase & { conditions: ParsedNodeBase[] },
  s: MagicString,
  context: ProcessContext
) {
  const { conditions } = node;

  let hasElse = false;

  const firstCondition = conditions[0];
  const lastCondition = conditions[conditions.length - 1];

  if (firstCondition) {
    const firstChild =
      firstCondition.children[0]?.type === "for"
        ? firstCondition.children[0]?.children[0]
        : firstCondition.children[0];
    const childStart = firstChild.node.loc.start.offset;
    s.prependLeft(childStart, "{");

    const lastChild =
      lastCondition.children[lastCondition.children.length - 1]?.type === "for"
        ? lastCondition.children[lastCondition.children.length - 1]?.children[
            lastCondition.children[lastCondition.children.length - 1]?.children
              .length - 1
          ]
        : lastCondition.children[lastCondition.children.length - 1];
    const childEnd = lastChild.node.loc.end.offset;
    s.appendRight(childEnd, "}");
  }

  for (let i = 0; i < conditions.length; i++) {
    const condition = conditions[i];

    const firstChild =
      condition.children[0]?.type === "for"
        ? condition.children[0]?.children[0]
        : condition.children[0];
    const lastChild =
      condition.children[condition.children.length - 1]?.type === "for"
        ? condition.children[condition.children.length - 1]?.children[
            condition.children[condition.children.length - 1]?.children.length -
              1
          ]
        : condition.children[condition.children.length - 1];

    const childStart = firstChild.node.loc.start.offset;
    const childEnd = lastChild.node.loc.end.offset;
    const conditionStart = condition.node.loc.start.offset;
    const conditionEnd = condition.node.loc.end.offset;

    const directiveStart = condition.node.loc.start.offset;
    const directiveEnd = directiveStart + condition.node.rawName.length + 1; // with =

    if (condition.directive !== "else") {
      // move v-if/v-else-if to before the first child
      s.move(conditionStart, conditionEnd, childStart);

      // add {  }
      //   s.appendLeft(childStart, "{");
      //   s.appendLeft(childEnd, "}");
    } /*else {
      // move v-if/v-else-if to before the first child
      s.move(conditionStart, conditionEnd, childStart + 1);
    }*/

    switch (condition.directive) {
      case "if": {
        // move v-if to after the expression

        // now the expression is at childStart
        s.move(directiveStart, directiveEnd, childStart);

        // replace v-if with ?
        s.overwrite(directiveStart, directiveEnd, "?");
        break;
      }
      case "else-if": {
        // now the expression is at childStart
        s.move(directiveStart, directiveEnd, childStart);

        // replace v-else-if with ?
        s.overwrite(directiveStart, directiveEnd, "?");
        break;
      }
      case "else": {
        // // now the expression is at childStart
        // directive end contains `=`
        s.move(directiveStart, directiveEnd - 1, childStart);

        // replave v-else with :
        s.overwrite(directiveStart, directiveEnd - 1, ":");
        // s.move(directiveStart, directiveStart + 1, childStart);
        hasElse = true;

        break;
      }
    }

    if ("expression" in condition && condition.expression) {
      const expression = condition.expression as ExpressionNode;

      // replace delimiters with ()
      s.overwrite(
        expression.loc.start.offset - 1,
        expression.loc.start.offset,
        (condition.directive === "else-if" ? ":" : "") + "("
      );
      s.overwrite(
        expression.loc.end.offset,
        expression.loc.end.offset + 1,
        ")"
      );

      retrieveStringExpressionNode(expression, s, context, true);
    }

    // s.overwrite(conditionStart - 1, conditionStart)

    // renderConditionNode(condition, s, context);

    // for (let i = 0; i < condition.children.length; i++) {
    //   const child = condition.children[i];
    //   renderNode(child, s, context);
    // }
    renderChildren(
      condition.children.map((x) => {
        if (x.type === "for") {
          // prevent wrapping of the for loop
          x.NO_WRAP = true;
        }
        return x;
      }),
      s,
      context
    );
  }

  if (!hasElse) {
    const lastCondition = conditions[conditions.length - 1];

    const lastChild =
      lastCondition.children[lastCondition.children.length - 1]?.type === "for"
        ? lastCondition.children[lastCondition.children.length - 1]?.children[
            lastCondition.children[lastCondition.children.length - 1]?.children
              .length - 1
          ]
        : lastCondition.children[lastCondition.children.length - 1];
    const childEnd = lastChild.node.loc.end.offset;
    s.prependRight(childEnd, " : undefined");
  }

  // function getContent(children: ParsedNodeBase[]) {
  //   return renderChildren(children, ignoredIdentifiers, false).map((x) => {
  //     // check if wrapped, if it is unwrap
  //     if (x[0] === "{" && x[x.length - 1] === "}") {
  //       x = x.slice(1);
  //       x = x.slice(0, -1);
  //     }
  //     return x;
  //   });
  // }
  // const conditions = (node.conditions as []).map((x) => {
  //   const condition = retriveStringExpressionNode(
  //     x.expression,
  //     ignoredIdentifiers
  //   );
  //   const content = getContent(x.children ?? []);
  //   return {
  //     rawName: x.rawName,
  //     condition,
  //     content,
  //   };
  // });
  // const c = conditions.reduce((prev, cur, currentIndex, arr) => {
  //   const last = currentIndex === arr.length - 1;
  //   if (cur.rawName !== "v-else") {
  //     const r = `${prev ? prev + " : " : ""}(${cur.condition}) ? ${
  //       cur.content
  //     }`;
  //     if (last) return r + " : undefined";
  //     return r;
  //   } else {
  //     return `${prev} : ${cur.content} `;
  //   }
  // }, "");
  // return `{ ${c} }`;
}

function renderComment(
  node: ParsedNodeBase,
  s: MagicString,
  context: ProcessContext
) {
  const commentOpenTag = "<!--";
  const commentCloseTag = "-->";
  const startingTag = node.node.loc.source.indexOf(commentOpenTag);
  const closingTag = node.node.loc.source.indexOf(commentCloseTag);

  s.overwrite(
    node.node.loc.start.offset + startingTag,
    node.node.loc.start.offset + startingTag + commentOpenTag.length,
    "{ /*"
  );

  s.overwrite(
    node.node.loc.start.offset + closingTag,
    node.node.loc.start.offset + closingTag + commentCloseTag.length,
    "*/ }"
  );
}
function renderInterpolation(
  node: ParsedNodeBase,
  s: MagicString,
  context: ProcessContext
) {
  // replace '{{' and '}}' with '{' and '}' to make valid TSX
  const interpolationNode = node.node as InterpolationNode;
  if (interpolationNode.loc.source.startsWith("{{")) {
    s.overwrite(
      interpolationNode.loc.start.offset,
      interpolationNode.loc.start.offset + 2,
      "{"
    );
  }
  if (interpolationNode.loc.source.endsWith("}}")) {
    s.overwrite(
      interpolationNode.loc.end.offset - 2,
      interpolationNode.loc.end.offset,
      "}"
    );
  }

  // process the content

  retrieveStringExpressionNode(interpolationNode.content, s, context);
}

function resolveComponentTag(
  node: PlainElementNode | ComponentNode | ElementNode,
  context: ProcessContext
): {
  name: string;
  accessor?: string;
} {
  if (node.tagType === ElementTypes.COMPONENT) {
    const camel = camelize(node.tag);
    // if not camel just return
    if (camel === node.tag) {
      return {
        name: node.tag,
        accessor: context.componentAccessor,
      };
    }
    // NOTE probably this is not 100% correct, maybe we could check if the component exists
    // by passing in the context
    return {
      name: capitalize(camel),
      accessor: context.componentAccessor,
    };
  }
  if (node.tagType === ElementTypes.SLOT) {
    return {
      name: "___VERTER_SLOT_COMP",
    };
  }
  return { name: node.tag };
}

const NonWordRegex = /\W/;

function retrieveStringExpressionNode(
  node: ExpressionNode | undefined,
  s: MagicString | undefined,
  context: ProcessContext,
  prepend = true,
  overrideOffset = -1,
  narrowConditions = false
) {
  if (!node) return undefined;
  switch (node.type) {
    case NodeTypes.SIMPLE_EXPRESSION: {
      if (node.isStatic) return node.content;

      // Not very complex condition
      if (!node.ast) {
        // TODO this should only be available on special content when
        // inside vscode
        if (!node.isStatic) {
          // this is probably partial element
          // while the user is typing is useful to keep parsing
          if (NonWordRegex.test(node.content)) {
            try {
              // const p = preprocessCode(node.content);
              const ast = acornParse(node.content, {
                ecmaVersion: "latest",
              });

              node.ast = ast.body[0];
            } catch (e) {
              console.error(e);
            }
          }
        }

        if (!node.ast) {
          return prepend ? appendCtx(node, s, context) : node.content;
        }
      }

      const offset =
        overrideOffset >= 0
          ? overrideOffset
          : node.loc.start.offset - node.ast.start;

      let trimmedOffset = 0;
      const trimmedContent = node.loc.source.trimStart();
      if (trimmedContent.length !== node.loc.source.length) {
        trimmedOffset = node.loc.source.length - trimmedContent.length;
      }

      return parseNodeText(
        node.ast,
        s,
        context,
        offset + trimmedOffset,
        prepend,
        narrowConditions
      );
    }
    case NodeTypes.COMPOUND_EXPRESSION: {
      return "NOT_KNOWN COMPOUND_EXPRESSION";
      break;
    }
    default: {
      // @ts-expect-error unknown type
      throw new Error(`Unknown expression type ${node.type}`);
    }
  }
}

function parseNodeText(
  node: _babel_types.Node | _babel_types.Node[],
  s: MagicString | undefined,
  context: ProcessContext,
  offset: number,
  prepend = true,
  narrowConditions = false
) {
  if (Array.isArray(node)) {
    return node.forEach((x) => parseNodeText(x, s, context, offset, prepend));
  }
  // if (offset > 0) {
  //   --offset;
  // }

  if ("params" in node) {
    const names = node.params.map((x) => x.content ?? x.name ?? x);

    if (names.length) {
      context = {
        ...context,
        ignoredIdentifiers: [...context.ignoredIdentifiers, ...names],
      };
    }
  }

  if ("expression" in node) {
    node.expression && parseNodeText(node.expression, s, context, offset);
  }
  if ("expressions" in node) {
    node.expressions &&
      node.expressions.forEach((x) =>
        parseNodeText(x, s, context, offset, prepend)
      );
  }

  if ("callee" in node) {
    // NOTE -1 is magic
    // node.callee && parseNodeText(node.callee, s, context, offset - 1, prepend);
    node.callee && parseNodeText(node.callee, s, context, offset, prepend);
  }

  if ("arguments" in node) {
    node.arguments &&
      node.arguments.forEach((p) =>
        parseNodeText(p, s, context, offset, prepend)
      );
  }

  if ("argument" in node) {
    node.argument && parseNodeText(node.argument, s, context, offset, prepend);
  }

  if ("exprName" in node) {
    node.exprName && parseNodeText(node.exprName, s, context, offset, prepend);
  }

  if ("properties" in node) {
    node.properties &&
      node.properties.forEach((p) =>
        parseNodeText(p, s, context, offset, prepend)
      );
  }

  if ("elements" in node) {
    node.elements &&
      node.elements.forEach((p) =>
        parseNodeText(p, s, context, offset, prepend)
      );
  }

  let testCondition = "";
  if ("test" in node) {
    if (node.test) {
      parseNodeText(node.test, s, context, offset, prepend);
      testCondition = s
        .snip(offset + node.test.start, offset + node.test.end)
        .toString();
    }
  }

  if ("consequent" in node) {
    const c = testCondition
      ? {
          ...context,
          conditions: {
            ...context.conditions,
            ifs: [...context.conditions.ifs, testCondition],
          },
        }
      : context;

    node.consequent &&
      parseNodeText(node.consequent, s, c, offset, prepend, narrowConditions);
  }
  if ("alternate" in node) {
    const c = testCondition
      ? {
          ...context,
          conditions: {
            ...context.conditions,
            elses: [...context.conditions.elses, testCondition],
          },
        }
      : context;
    node.alternate &&
      parseNodeText(node.alternate, s, c, offset, prepend, narrowConditions);
  }

  if ("left" in node) {
    node.left && parseNodeText(node.left, s, context, offset);
  }
  if ("right" in node) {
    node.right && parseNodeText(node.right, s, context, offset);
  }

  if ("body" in node) {
    node.body && parseNodeText(node.body, s, context, offset);
  }

  switch (node.type) {
    case "CallExpression": {
      const callee = node.callee;
      break;
    }
    case "Identifier": {
      prepend && appendCtx(node, s, context, offset);
      break;
    }
    case "OptionalMemberExpression":
    case "MemberExpression": {
      node.object && parseNodeText(node.object, s, context, offset);
      // computed is true if is access []
      node.property &&
        parseNodeText(node.property, s, context, offset, node.computed);
      break;
    }
    case "LogicalExpression": {
      break;
    }
    case "ObjectExpression": {
      break;
    }
    case "TSSatisfiesExpression": {
      break;
    }
    case "TSAsExpression": {
      node.typeAnnotation &&
        parseNodeText(node.typeAnnotation, s, context, offset);
      break;
    }
    case "TSTypeReference": {
      break;
    }
    case "BinaryExpression": {
      break;
    }
    case "ConditionalExpression": {
      break;
    }
    case "StringLiteral": {
      break;
    }
    case "NumericLiteral": {
      break;
    }
    case "BooleanLiteral": {
      break;
    }
    case "TemplateLiteral": {
      break;
    }
    case "NullLiteral": {
      break;
    }
    case "UnaryExpression": {
      break;
    }
    case "FunctionExpression":
    case "ArrowFunctionExpression": {
      if (narrowConditions) {
        const hasBlock = node.body.type === "BlockStatement";
        const bodyStartOffset =
          offset + (hasBlock ? node.body.body[0].start : node.body.start);

        const narrowCondition = generateNarrowCondition(context, hasBlock);

        if (narrowCondition) {
          s.prependLeft(bodyStartOffset, narrowCondition);
        }
      }
      break;
    }
    case "ObjectProperty": {
      if (node.shorthand) {
        const v = appendCtx(node.key, undefined, context);

        s.appendLeft(offset + node.key.loc.end.index, `:${v}`);
      } else {
        if (node.key.type !== "Identifier") {
          parseNodeText(node.key, s, context, offset);
        }
        node.value && parseNodeText(node.value, s, context, offset);
      }
      break;
    }
    default: {
      break;
    }
  }
}

function generateNarrowCondition(context: ProcessContext, isBlock = false) {
  const toNegate = context.conditions.ifs.join(" && ");
  const conditions = [
    toNegate ? `!(${toNegate})` : "",
    ...context.conditions.elses,
  ]
    .filter(Boolean)
    .join(" || ");

  if (!conditions) return "";

  if (isBlock) {
    return `if(${conditions}) { return; } `;
  }
  return `${conditions} ? undefined : `;
}

const StaticNodeTypes = new Set<keyof _babel_types.ParentMaps>([
  //   "StringLiteral",
  //   "NumericLiteral",
  //   "NullLiteral",
]);

function appendCtx(
  node: SimpleExpressionNode | _babel_types.Expression | string,
  s: MagicString | undefined,
  context: ProcessContext,

  /**
   * only used for the offset when using babel
   */
  __offset: number = 0
) {
  // babel parses `.` at the end as the name instead of member expression,
  // when typing we don't want to attach the ctx if end in a .
  const originalContent = node.content ?? node.name ?? node;
  const content = originalContent.split(".")[0];
  if (
    isGloballyAllowed(content) ||
    isLiteralWhitelisted(content) ||
    ~context.ignoredIdentifiers.indexOf(content) ||
    !context.accessor
  )
    return content;

  if (StaticNodeTypes.has(node.type) || node.type?.endsWith?.("Literal"))
    return content;

  const accessor = context.accessor;
  if (s) {
    // node.loc.start.index is babel accessing, the first char pos is 1
    // instead of zero-based
    const start =
      (node.loc
        ? node.loc.start.offset ?? node.loc.start.index
        : node.start ?? 0) + __offset;
    const end =
      (node.loc ? node.loc.end.offset ?? node.loc.end.index : node.end ?? 0) +
      __offset;

    if (start === end) {
      console.warn("VERTER same start/end", originalContent, start, end);
      return;
    }

    // there's a case where the offset is off
    if (s.original.slice(start, end) !== originalContent) {
      s.prependRight(start + 1, `${accessor}.`);
    } else {
      s.prependRight(start, `${accessor}.`);
    }
  }

  return `${accessor}.${content}`;
}

function* retrieveAccessors(
  node:
    | _babel_types.BinaryExpression
    | _babel_types.Identifier
    | _babel_types.PrivateName
    | _babel_types.Expression
    | _babel_types.ObjectPattern
    | _babel_types.ObjectProperty
    | _babel_types.RestElement,
  context: ProcessContext
) {
  switch (node.type) {
    case "BinaryExpression": {
      yield* retrieveAccessors(node.left, context);
      yield* retrieveAccessors(node.right, context);
      break;
    }
    case "Identifier": {
      yield appendCtx(node, undefined, context);
      break;
    }
    case "PrivateName": {
      yield* retrieveAccessors(node.id, context);
      break;
    }
    case "MemberExpression": {
      const obj = retrieveAccessors(node.object, context).next().value;
      const property = node.property.name;
      yield appendCtx(`${obj}.${property}`, undefined, context);
      break;
    }
    case "ObjectPattern": {
      for (const it of node.properties) {
        yield* retrieveAccessors(it, context);
      }
      break;
    }

    case "ObjectProperty": {
      // if (node.shorthand) {
      yield* retrieveAccessors(node.shorthand ? node.key : node.value, {
        ...context,
        accessor: undefined,
      });
      // }

      break;
    }

    case "RestElement": {
      yield* retrieveAccessors(node.argument, {
        ...context,
        accessor: undefined,
      });
      break;
    }
  }
}
