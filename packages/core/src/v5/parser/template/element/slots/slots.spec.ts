import { parse as parseSFC } from "@vue/compiler-sfc";
import { SlotsContext, handleSlot } from "./slots.js";
import { ElementNode, ElementTypes, NodeTypes } from "@vue/compiler-core";
import { TemplateTypes } from "../../types.js";

describe("parser template slots", () => {
  describe("element", () => {
    function parse(
      content: string,
      context: SlotsContext = { ignoredIdentifiers: [] }
    ) {
      const source = `<template>${content}</template>`;

      const sfc = parseSFC(source, {});

      const template = sfc.descriptor.template;
      const ast = template ? template.ast : null;

      const result = handleSlot(ast.children[0] as any, context);

      return {
        source,
        result,
      };
    }

    it("return null if not element", () => {
      const node = {
        type: NodeTypes.COMMENT,
      } as any as ElementNode;
      expect(handleSlot(node, { ignoredIdentifiers: [] })).toBeNull();
    });
    it("return null if node type is not <slot/>", () => {
      expect(parse(`<div/>`).result).toBeNull();
    });

    it("no name", () => {
      const { result } = parse(`<slot></slot>`);
      expect(result).toMatchObject({
        slot: {
          type: TemplateTypes.Slot,
          node: {
            type: NodeTypes.ELEMENT,
            tagType: ElementTypes.SLOT,
          },
          name: null,
          props: null,
          parent: null,
        },

        context: { ignoredIdentifiers: [] },

        items: [
          {
            type: TemplateTypes.Slot,
          },
        ],
      });
    });

    it("slot name='test'", () => {
      const { result } = parse(`<slot name='test'/>`);
      expect(result).toMatchObject({
        slot: {
          type: TemplateTypes.Slot,
          node: {
            type: NodeTypes.ELEMENT,
            tagType: ElementTypes.SLOT,
          },
          name: {
            type: TemplateTypes.Prop,
            name: "name",
            value: "test",
            node: {
              type: NodeTypes.ATTRIBUTE,
              name: "name",
            },
          },
          props: null,
          parent: null,
        },
        context: { ignoredIdentifiers: [] },
        items: [
          {
            type: TemplateTypes.Slot,
            name: {
              name: "name",
              value: "test",
            },
          },
        ],
      });
    });

    it('slot :name="test"', () => {
      const { result } = parse(`<slot :name="test"/>`);
      expect(result).toMatchObject({
        slot: {
          type: TemplateTypes.Slot,
          node: {
            type: NodeTypes.ELEMENT,
            tagType: ElementTypes.SLOT,
          },
          name: {
            type: TemplateTypes.Prop,
            name: [
              {
                type: TemplateTypes.Binding,
                name: "name",
                ignore: true,
              },
            ],
            node: {
              type: NodeTypes.DIRECTIVE,
              name: "bind",
            },
          },
          props: null,
          parent: null,
        },
        context: { ignoredIdentifiers: [] },
        items: [
          {
            type: TemplateTypes.Slot,
            name: {
              name: [
                {
                  type: TemplateTypes.Binding,
                  name: "name",
                  ignore: true,
                },
              ],
            },
          },
          {
            type: TemplateTypes.Binding,
            name: "name",
            ignore: true,
          },

          {
            type: TemplateTypes.Binding,
            name: "test",
            ignore: false,
          },
        ],
      });
    });

    it('slot v-bind:name="test"', () => {
      const { result } = parse(`<slot v-bind:name="test"/>`);
      expect(result).toMatchObject({
        slot: {
          type: TemplateTypes.Slot,
          node: {
            type: NodeTypes.ELEMENT,
            tagType: ElementTypes.SLOT,
          },
          name: {
            type: TemplateTypes.Prop,
            name: [
              {
                type: TemplateTypes.Binding,
                name: "name",
                ignore: true,
              },
            ],
            node: {
              type: NodeTypes.DIRECTIVE,
              name: "bind",
            },
          },
          props: null,
          parent: null,
        },
        context: { ignoredIdentifiers: [] },
        items: [
          {
            type: TemplateTypes.Slot,
            name: {
              name: [
                {
                  type: TemplateTypes.Binding,
                  name: "name",
                  ignore: true,
                },
              ],
            },
          },
          {
            type: TemplateTypes.Binding,
            name: "name",
            ignore: true,
          },

          {
            type: TemplateTypes.Binding,
            name: "test",
            ignore: false,
          },
        ],
      });
    });

    it.only("slot test", () => {
      const { result } = parse(`<slot test="1"/>`);
      expect(result).toMatchObject({
        slot: {
          type: TemplateTypes.Slot,
          node: {
            type: NodeTypes.ELEMENT,
            tagType: ElementTypes.SLOT,
          },
          name: null,
          props: null,
          parent: null,
        },
        context: { ignoredIdentifiers: [] },
        items: [
          {
            type: TemplateTypes.Slot,
          },
        ],
      });
    });
  });
});
