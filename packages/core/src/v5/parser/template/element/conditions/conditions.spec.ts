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
            // Note: "true" is a static literal, so it's correctly marked as ignore
            ignore: true,
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

  describe("multiple", () => {
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

      const results = ast!.children.map((x) =>
        handleConditions(x as any, ast!, context)
      );

      return {
        source,
        results,

        items: results.flatMap((x) => x?.items),
      };
    }
    it("v-if v-else", () => {
      const { items } = parse(
        `<div v-if="true" />
          <div v-else />`
      );

      expect(items).toMatchObject([
        {
          type: TemplateTypes.Condition,
          node: {
            name: "if",
          },
          siblings: [],
        },
        {
          type: TemplateTypes.Binding,
          name: "true",
        },
        {
          type: TemplateTypes.Condition,
          node: {
            name: "else",
          },
          siblings: [
            {
              type: TemplateTypes.Condition,
              node: {
                name: "if",
              },
              siblings: [],
            },
          ],
        },
      ]);
    });
    it("v-if v-else-if", () => {
      const { items } = parse(
        `<div v-if="true" />
          <div v-else-if="true" />`
      );

      expect(items).toMatchObject([
        {
          type: TemplateTypes.Condition,
          node: {
            name: "if",
          },
          siblings: [],
        },
        {
          type: TemplateTypes.Binding,
          name: "true",
        },
        {
          type: TemplateTypes.Condition,
          node: {
            name: "else-if",
          },
          siblings: [
            {
              type: TemplateTypes.Condition,
              node: {
                name: "if",
              },
              siblings: [],
            },
          ],
        },
        {
          type: TemplateTypes.Binding,
          name: "true",
        },
      ]);
    });
    it("v-if v-else-if v-else", () => {
      const { items } = parse(
        `<div v-if="true" />
          <div v-else-if="false" />
          <div v-else />`
      );

      expect(items).toMatchObject([
        {
          type: TemplateTypes.Condition,
          node: {
            name: "if",
          },
          siblings: [],
        },
        {
          type: TemplateTypes.Binding,
          name: "true",
        },
        {
          type: TemplateTypes.Condition,
          node: {
            name: "else-if",
          },
          siblings: [
            {
              type: TemplateTypes.Condition,
              node: {
                name: "if",
              },
              siblings: [],
            },
          ],
        },
        {
          type: TemplateTypes.Binding,
          name: "false",
        },
        {
          type: TemplateTypes.Condition,
          node: {
            name: "else",
          },
          siblings: [
            {
              type: TemplateTypes.Condition,
              node: {
                name: "if",
              },
              siblings: [],
            },
            {
              type: TemplateTypes.Condition,
              node: {
                name: "else-if",
              },
              siblings: [
                {
                  type: TemplateTypes.Condition,
                  node: {
                    name: "if",
                  },
                  siblings: [],
                },
              ],
            },
          ],
        },
      ]);
    });
  });
});
