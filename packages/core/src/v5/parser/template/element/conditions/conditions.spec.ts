import {
  parse as parseSFC,
  compileTemplate,
  extractRuntimeProps,
} from "@vue/compiler-sfc";
import { handleConditions, ConditionsContext } from "./index.js";
import { ElementNode, NodeTypes } from "@vue/compiler-core";
import { TemplateCondition, TemplateTypes } from "../../types.js";

describe("parser template conditions", () => {
  function parse(
    content: string,
    context: ConditionsContext = {
      ignoredIdentifiers: [],
      conditions: [],
    }
  ) {
    const source = `<template>${content}</template>`;

    const sfc = parseSFC(source, {});

    const template = sfc.descriptor.template;
    const ast = template?.ast!;

    const result = handleConditions(ast!.children[0] as any, ast!, context);

    return {
      source,
      result,
    };
  }

  it("return null if not element", () => {
    const node = {
      type: NodeTypes.COMMENT,
    } as any as ElementNode;

    // @ts-expect-error
    expect(parse(node, {}).result).toBeNull();
  });

  it("return null if no condition", () => {
    const { result } = parse("<div></div>");

    expect(result).toBeNull();
  });

  it.each(["if", "else-if", "else"])("%s", (name) => {
    const { result } = parse(`<div v-${name}="true"></div>`);

    expect(result).toMatchObject({
      condition: {
        type: TemplateTypes.Condition,
        node: {
          type: NodeTypes.DIRECTIVE,
          name,
          rawName: "v-" + name,
        },

        element: {
          type: NodeTypes.ELEMENT,
          tag: "div",
        },

        parent: {},

        bindings: [
          {
            type: TemplateTypes.Binding,
            node: {
              type: NodeTypes.SIMPLE_EXPRESSION,
              content: "true",
            },
            name: "true",
            ignore: false,
          },
        ],

        context: {
          ignoredIdentifiers: [],
          conditions: [],
        },
      },
      items: [
        {
          type: TemplateTypes.Condition,
        },
        {
          type: TemplateTypes.Binding,
          name: "true",
        },
      ],
      context: {
        ignoredIdentifiers: [],
        conditions: [
          {
            type: TemplateTypes.Condition,
            node: {
              type: NodeTypes.DIRECTIVE,
            },

            element: {
              type: NodeTypes.ELEMENT,
            },

            parent: {},

            bindings: [
              {
                type: TemplateTypes.Binding,
              },
            ],
          },
        ],
      },
    });
  });

  it("add contitions to context", () => {
    const { result } = parse(`<div v-if="true"></div>`, {
      ignoredIdentifiers: [],
      conditions: [
        {
          type: TemplateTypes.Condition,
          // @ts-expect-error
          test: 1,
        },
      ],

      random: 1,
    });

    expect(result).toMatchObject({
      condition: {
        context: {
          random: 1,
          ignoredIdentifiers: [],
          conditions: [
            {
              type: TemplateTypes.Condition,
              test: 1,
            },
          ],
        },
      },

      context: {
        random: 1,
        ignoredIdentifiers: [],
        conditions: [
          {
            type: TemplateTypes.Condition,
            test: 1,
          },
          {
            type: TemplateTypes.Condition,
          },
        ],
      },
    });
  });
});
