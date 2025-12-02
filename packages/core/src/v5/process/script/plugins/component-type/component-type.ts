/**
 * @fileoverview Component Type Plugin for Vue SFC to TSX transformation.
 *
 * This plugin generates typed component type functions from Vue templates,
 * enabling full TypeScript type inference for template elements, including:
 * - HTML elements with their proper element types (e.g., HTMLDivElement)
 * - Vue components with their props and slots
 * - Loop variables from v-for directives
 * - Scoped slot props from parent components
 * - Conditional narrowing from v-if/v-else-if/v-else
 *
 * @example Generated output for a simple template:
 * ```vue
 * <template>
 *   <div id="app">{{ message }}</div>
 * </template>
 * ```
 * Generates:
 * ```typescript
 * function ___VERTER___Comp0() {
 *   const { message } = {} as ___VERTER___FullContext;
 *   return enhanceElementWithProps({} as HTMLElementTagNameMap["div"], { "id": "app" });
 * }
 * ```
 *
 * @see {@link file://packages/types/src/components/components.ts} - Type helpers used by generated code
 * @see {@link file://packages/types/src/loops/loops.ts} - Loop extraction helpers
 * @see {@link file://packages/types/src/slots/slots.ts} - Slot argument extraction helpers
 */

import { definePlugin, ScriptContext } from "../../types";
import { BundlerHelper } from "../../../template/helpers/bundler";
import {
  createHelperImport,
  generateImport,
  VERTER_HELPERS_IMPORT,
} from "../../../utils";
import { type AvailableExports } from "@verter/types/string";
import {
  TemplateTypes,
  type TemplateElement,
} from "../../../../parser/template/types";
import {
  ParsedBlockTemplate,
  ProcessItemType,
  ScriptTypes,
  VerterASTNode,
} from "../../../..";
import {
  AttributeNode,
  DirectiveNode,
  ElementNode,
  ElementTypes,
  NodeTypes,
} from "@vue/compiler-core";
import {
  ConditionalPlugin,
  generateConditionText,
} from "../../../template/plugins";

/**
 * Plugin that generates typed component type functions from Vue templates.
 *
 * This plugin runs in the "pre" phase to set up conditional narrowing,
 * then in the "post" phase to generate the actual component type functions.
 *
 * ## How it works:
 *
 * 1. **Pre-phase**: Processes v-if/v-else-if/v-else conditions to enable
 *    type narrowing in the generated code.
 *
 * 2. **Post-phase**: For each element in the template, generates a function
 *    that returns the properly typed instance:
 *    - HTML elements use `enhanceElementWithProps` with `HTMLElementTagNameMap`
 *    - Vue components use `new ComponentName(props)` syntax
 *    - Loops use `extractLoops` to type `v-for` variables
 *    - Slots use `extractArgumentsFromRenderSlot` to type scoped slot props
 *
 * ## Generated helpers used:
 *
 * - `enhanceElementWithProps<T, P>`: Merges element type with additional props
 * - `extractLoops<T>`: Extracts key/value types from array or object iterations
 * - `extractArgumentsFromRenderSlot<T, N>`: Extracts slot props from parent component
 *
 * @example Component with v-for loop:
 * ```vue
 * <template>
 *   <li v-for="(item, index) in items" :key="index">{{ item.name }}</li>
 * </template>
 * ```
 * Generates:
 * ```typescript
 * function ___VERTER___Comp0() {
 *   const { items } = {} as ___VERTER___FullContext;
 *   const { key: index, value: item } = extractLoops(items);
 *   return enhanceElementWithProps({} as HTMLElementTagNameMap["li"], { "key": index });
 * }
 * ```
 */
export const ComponentTypePlugin = definePlugin({
  name: "VerterComponentType",
  enforce: "pre",

  narrowByParent: new Map<TemplateElement, string[]>(),

  pre(s, ctx) {
    this.narrowByParent.clear();
    const template = ctx.blocks.find((x) => x.type === "template") as
      | ParsedBlockTemplate
      | undefined;

    if (!template) return;

    template.result.items
      .filter((x) => x.type === TemplateTypes.Element)
      .map((el) => {
        if (!el.condition) return;

        ConditionalPlugin.transformCondition(el.condition, ctx.s, {
          narrow: true,
        } as any);
      });
    // ConditionalPlugin.transformCondition(element.condition, ctx.s, {
    //   narrow: true,
    // });
  },

  post(s, ctx) {
    const template = ctx.blocks.find((x) => x.type === "template") as
      | ParsedBlockTemplate
      | undefined;

    if (!template) return;

    ctx.items.push(
      createHelperImport(
        [
          "enhanceElementWithProps",
          "extractLoops",
          "extractArgumentsFromRenderSlot",
        ],
        ctx.prefix
      )
    );

    const components = template.result.items.filter(
      (x) => x.type === TemplateTypes.Element
    ) as TemplateElement[];

    const imports = new Set(
      ctx.block.result?.items
        .filter((x) => x.type === ScriptTypes.Import)
        .flatMap((x) => x.bindings)
        .map((x) => x.name)
    );

    const templateBindings = new Set<string>(
      template.result.items
        .map((x) =>
          x.type === TemplateTypes.Binding && x.name && !imports.has(x.name)
            ? x.name
            : null
        )
        .filter(Boolean) as string[]
    );

    const extraContext = {} as any;

    const allComponents = components.map((x) =>
      resolveComponent(x, templateBindings, extraContext, ctx)
    );

    const rootComponent = template.result.items.filter(
      (x) =>
        x.type === TemplateTypes.Element &&
        x.parent === template.block.block.ast
    ) as TemplateElement[];

    s.append(
      `function getRootComponent${ctx.generic ? ctx.generic.source : ""}() {`
    );

    if (rootComponent.length === 1) {
      s.append(
        `return Comp${rootComponent[0].node.loc.start.offset}${
          ctx.generic ? ctx.generic.source : ""
        }()`
      );
    } else {
      s.append(`return {};`);
    }
    s.append(`}\n`);

    s.append("\n" + allComponents.join("\n"));

    s.append(`\n`);
  },
});

/**
 * Generates a unique component function name based on the element's source offset.
 *
 * @param node - The element node from the template AST
 * @param ctx - The script context containing the prefix function
 * @returns A prefixed function name like `___VERTER___Comp123`
 */
function resolveComponentNameByNode(node: ElementNode, ctx: ScriptContext) {
  const start = node.loc.start;
  return ctx.prefix(`Comp${start.offset}`);
}

/**
 * Determines whether a template element represents a Vue component or a plain HTML element.
 *
 * An element is considered a component if:
 * - It has a tagType of COMPONENT (e.g., PascalCase or registered component)
 * - It's an element whose tag matches a binding in the script context
 *
 * @param element - The template element to check
 * @param ctx - The script context containing bindings
 * @returns true if the element is a Vue component, false if it's a plain HTML element
 */
function isComponent(element: TemplateElement, ctx: ScriptContext) {
  const node = element.node;
  const name = node.tag;

  return (
    node.tagType === ElementTypes.COMPONENT ||
    (node.tagType === ElementTypes.ELEMENT &&
      ctx.items.find(
        (x) => x.type === ProcessItemType.Binding && x.name === name
      ))
  );
}

/**
 * Converts a prop (attribute or directive) to a TypeScript object property string.
 *
 * @example Static attribute:
 * ```html
 * <div id="app">
 * ```
 * Returns: `"id": "app"`
 *
 * @example Dynamic binding:
 * ```html
 * <div :class="myClass">
 * ```
 * Returns: `"class": myClass`
 *
 * @param prop - The attribute or directive node
 * @param ctx - The script context
 * @returns A string like `"propName": value` or empty string if not applicable
 */
function propToString(
  prop: AttributeNode | DirectiveNode,
  ctx: ScriptContext
): string {
  if (prop.type === NodeTypes.ATTRIBUTE /* ATTRIBUTE */) {
    return `"${prop.name}": ${JSON.stringify(
      prop.value ? prop.value.content : true
    )}`;
  } else if (prop.type === NodeTypes.DIRECTIVE /* DIRECTIVE */) {
    if (
      prop.arg &&
      prop.arg.type === NodeTypes.SIMPLE_EXPRESSION /* SIMPLE_EXPRESSION */
    ) {
      const argContent = prop.arg.content;
      if (prop.exp && prop.exp.type === 4 /* SIMPLE_EXPRESSION */) {
        const expContent = prop.exp.content;
        return `"${argContent}": ${expContent}`;
      } else {
        return `"${argContent}": true`;
      }
    }
  }
  return "";
}

/**
 * Collects all props from an element and formats them as a TypeScript object literal.
 *
 * @param element - The template element
 * @param ctx - The script context
 * @returns A string like `{ "id": "app", "class": myClass }`
 */
function resolveComponentProps(element: TemplateElement, ctx: ScriptContext) {
  const node = element.node;

  const props = node.props
    .map((p) => propToString(p, ctx))
    .filter((x) => x.length > 0)
    .join(", ");

  return `{${props}}`;
}

/**
 * Generates a complete component type function for a template element.
 *
 * For Vue components, generates:
 * ```typescript
 * function ___VERTER___Comp0() {
 *   // context setup (bindings, loops, conditionals)
 *   return new MyComponent({ "prop": value });
 * }
 * ```
 *
 * For HTML elements, generates:
 * ```typescript
 * function ___VERTER___Comp0() {
 *   // context setup
 *   return enhanceElementWithProps({} as HTMLElementTagNameMap["div"], { "id": "app" });
 * }
 * ```
 *
 * @param element - The template element to process
 * @param templateBindings - Set of binding names available in the template
 * @param extraContext - Shared context for tracking narrowing and loop bindings
 * @param ctx - The script context
 * @returns The generated function as a string
 */
function resolveComponent(
  element: TemplateElement,
  templateBindings: Set<string>,
  extraContext: any,
  ctx: ScriptContext
) {
  const node = element.node;
  const tag = node.tag;
  const isComp = isComponent(element, ctx);
  const name = resolveComponentNameByNode(node, ctx);
  const props = resolveComponentProps(element, ctx);
  const pre = availableContext(element, templateBindings, extraContext, ctx);

  return `function ${name}${ctx.generic ? ctx.generic.source : ""}() {
${pre}
  return ${
    isComp
      ? `new ${tag}(${props})`
      : `${"enhanceElementWithProps" as AvailableExports}(${tagToHTMLElement(
          tag
        )},${props})`
  }  
}`;
}

/**
 * Converts an HTML tag name to a TypeScript type assertion expression.
 *
 * @param tag - The HTML tag name (e.g., "div", "span", "input")
 * @returns A type assertion string like `{} as HTMLElementTagNameMap["div"]`
 */
function tagToHTMLElement(tag: string): string {
  // {} as ${tagToHTMLElement(tag)}
  return `{} as HTMLElementTagNameMap["${tag}"]`;
}

/**
 * Generates the context setup code for a component type function.
 *
 * This includes:
 * - Extracting bindings from FullContext for template variables
 * - Adding v-for loop variable extractions (key/value from extractLoops)
 * - Adding scoped slot prop extractions (from extractArgumentsFromRenderSlot)
 * - Adding conditional narrowing statements (if conditions)
 *
 * @example Generated context for v-for:
 * ```typescript
 * const { items } = {} as ___VERTER___FullContext;
 * const { key: index, value: item } = extractLoops(items);
 * ```
 *
 * @example Generated context for scoped slot:
 * ```typescript
 * const { msg } = extractArgumentsFromRenderSlot(Comp0(), "default");
 * ```
 *
 * @param element - The template element being processed
 * @param templateBindings - Set of binding names used in the template
 * @param extraContext - Shared context for accumulating narrowing/loop bindings
 * @param ctx - The script context
 * @returns Generated setup code as a string
 */
function availableContext(
  element: TemplateElement,
  templateBindings: Set<string>,
  extraContext: any,
  ctx: ScriptContext
) {
  const bindings = ctx.items
    .map((x) =>
      x.type === ProcessItemType.Binding &&
      x.name &&
      templateBindings.has(x.name)
        ? x.name
        : null
    )
    .filter(Boolean);

  const bStr =
    bindings?.length > 0
      ? `const {${bindings?.join(",") ?? ""}}={} as ${ctx.prefix(
          "FullContext"
        )}${ctx.generic ? `<${ctx.generic.names.join(",")}>` : ""};`
      : "";

  if (extraContext.narrowBindings === undefined) {
    extraContext.narrowBindings = [];
  }
  resolveNarrow(element, extraContext, ctx);

  if (element.loop) {
    let item = "";
    const forLoop = element.loop?.node.forParseResult;
    if (forLoop) {
      const bindings = [
        (forLoop.key || forLoop.index)?.loc.source
          ? `key:${(forLoop.key || forLoop.index)?.loc.source}`
          : "",
        forLoop.value?.loc.source ? `value:${forLoop.value?.loc.source}` : "",
      ].filter(Boolean);
      item = `const {${bindings.join(",")}}=${ctx.prefix(
        "extractLoops" as AvailableExports
      )}(${forLoop.source.loc.source});`;
    }

    if (element.loop) {
      extraContext.narrowBindings.push({
        start: element.node.loc.start.offset,
        end: element.node.loc.end.offset,
        content: item,
      });
    }
  }

  const slotProp = element.node.props.find(
    (x) => x.name === "slot" && x.type === NodeTypes.DIRECTIVE
  ) as DirectiveNode | undefined;
  if (
    element.slot &&
    element.slot.type === TemplateTypes.SlotRender &&
    slotProp &&
    slotProp.exp
  ) {
    const slot = element.slot;
    const parentName = `Comp${slot.parent?.loc.start.offset}`;

    const parentRetriever = `${parentName}${
      ctx.generic ? `<${ctx.generic.names.join(",")}>` : ""
    }()`;

    const content = `const ${slotProp.exp.loc.source}=${ctx.prefix(
      "extractArgumentsFromRenderSlot" as AvailableExports
    )}(${parentRetriever},"${
      Array.isArray(slot.name) ? slot.name[0].name : slot.name
    }");`;

    extraContext.narrowBindings.push({
      start: element.node.loc.start.offset,
      end: element.node.loc.end.offset,
      content,
    });
  }

  const matchedLoops = extraContext.narrowBindings
    .filter(
      (x: { start: number; end: number; content: string }) =>
        x.start <= element.node.loc.start.offset &&
        x.end >= element.node.loc.end.offset
    )
    .map((x: { content: string }) => x.content);

  return [bStr, ...matchedLoops].filter(Boolean).join("\n");
}

/**
 * Generates conditional narrowing code for v-if/v-else-if/v-else elements.\n *\n * When an element has conditions (from v-if/v-else-if), this generates\n * early return statements that help TypeScript narrow types.\n *\n * @example For `<div v-if=\"user\">...</div>`:\n * ```typescript\n * if(!(user)) return null;\n * ```\n *\n * @param element - The template element with conditional rendering\n * @param extraContext - Shared context for accumulating narrowing bindings\n * @param ctx - The script context\n * @returns Generated narrowing code or undefined if no condition\n */
function resolveNarrow(
  element: TemplateElement,
  extraContext: any,
  ctx: ScriptContext
) {
  if (!element.condition) return;

  if (element.context.conditions.length > 0) {
    const condition = generateConditionText(element.context.conditions, ctx.s);
    const finalCondition = `if(!(${condition})) return null;`;

    if (!extraContext.narrowBindings) {
      extraContext.narrowBindings = [];
    }
    extraContext.narrowBindings.push({
      start: element.node.loc.start.offset,
      end: element.node.loc.end.offset,
      content: finalCondition,
    });

    const conditions = [finalCondition];

    return conditions.join("\n");
  }
}
