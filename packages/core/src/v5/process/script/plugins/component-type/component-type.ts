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

function resolveComponentNameByNode(node: ElementNode, ctx: ScriptContext) {
  const start = node.loc.start;
  return ctx.prefix(`Comp${start.offset}`);
}

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

function resolveComponentProps(element: TemplateElement, ctx: ScriptContext) {
  const node = element.node;

  const props = node.props
    .map((p) => propToString(p, ctx))
    .filter((x) => x.length > 0)
    .join(", ");

  return `{${props}}`;
}

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

function tagToHTMLElement(tag: string): string {
  // {} as ${tagToHTMLElement(tag)}
  return `{} as HTMLElementTagNameMap["${tag}"]`;
}

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
      (x) =>
        x.start <= element.node.loc.start.offset &&
        x.end >= element.node.loc.end.offset
    )
    .map((x) => x.content);

  return [bStr, ...matchedLoops].filter(Boolean).join("\n");
}

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
