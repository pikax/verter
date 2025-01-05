import { parse as parseSFC } from "@vue/compiler-sfc";
import {
  SlotsContext,
  handleSlotDeclaration,
  handleSlotProp,
} from "./slots.js";
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
      const ast = template?.ast!;

      const result = handleSlotDeclaration(ast.children[0] as any, context);

      return {
        source,
        result,
      };
    }

    it("return null if not element", () => {
      const node = {
        type: NodeTypes.COMMENT,
      } as any as ElementNode;
      expect(
        handleSlotDeclaration(node, { ignoredIdentifiers: [] })
      ).toBeNull();
    });
    it("return null if node type is not <slot/>", () => {
      expect(parse(`<div/>`).result).toBeNull();
    });

    it("no name", () => {
      const { result } = parse(`<slot></slot>`);
      expect(result).toMatchObject({
        slot: {
          type: TemplateTypes.SlotDeclaration,
          node: {
            type: NodeTypes.ELEMENT,
            tagType: ElementTypes.SLOT,
          },
          name: null,
          props: [],
          parent: null,
        },

        context: { ignoredIdentifiers: [] },

        items: [
          {
            type: TemplateTypes.SlotDeclaration,
          },
        ],
      });
    });

    it("slot name='test'", () => {
      const { result } = parse(`<slot name='test'/>`);
      expect(result).toMatchObject({
        slot: {
          type: TemplateTypes.SlotDeclaration,
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
          props: [
            {
              type: TemplateTypes.Prop,
              name: "name",
              value: "test",
              node: {
                type: NodeTypes.ATTRIBUTE,
                name: "name",
              },
            },
          ],
          parent: null,
        },
        context: { ignoredIdentifiers: [] },
        items: [
          {
            type: TemplateTypes.SlotDeclaration,
            name: {
              name: "name",
              value: "test",
            },
          },
          {
            type: TemplateTypes.Prop,
            name: "name",
            value: "test",
            node: {
              type: NodeTypes.ATTRIBUTE,
              name: "name",
            },
          },
        ],
      });
    });

    it('slot :name="test"', () => {
      const { result } = parse(`<slot :name="test"/>`);
      expect(result).toMatchObject({
        slot: {
          type: TemplateTypes.SlotDeclaration,
          node: {
            type: NodeTypes.ELEMENT,
            tagType: ElementTypes.SLOT,
          },
          name: {
            type: TemplateTypes.Binding,
            name: "name",
            ignore: true,
          },
          props: [
            {
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
                rawName: ":name",
              },
            },
          ],
          parent: null,
        },
        context: { ignoredIdentifiers: [] },
        items: [
          {
            type: TemplateTypes.SlotDeclaration,
            name: {
              type: TemplateTypes.Binding,
              name: "name",
              ignore: true,
            },
          },
          {
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
              rawName: ":name",
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
          type: TemplateTypes.SlotDeclaration,
          node: {
            type: NodeTypes.ELEMENT,
            tagType: ElementTypes.SLOT,
          },
          name: {
            type: TemplateTypes.Binding,
            name: "name",
            ignore: true,
          },
          props: [
            {
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
                rawName: "v-bind:name",
              },
            },
          ],
          parent: null,
        },
        context: { ignoredIdentifiers: [] },
        items: [
          {
            type: TemplateTypes.SlotDeclaration,
            name: {
              type: TemplateTypes.Binding,
              name: "name",
              ignore: true,
            },
          },
          {
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
              rawName: "v-bind:name",
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

    it("slot test", () => {
      const { result } = parse(`<slot test="1"/>`);
      expect(result).toMatchObject({
        slot: {
          type: TemplateTypes.SlotDeclaration,
          node: {
            type: NodeTypes.ELEMENT,
            tagType: ElementTypes.SLOT,
          },
          name: null,
          props: [
            {
              type: TemplateTypes.Prop,
              name: "test",
              value: "1",
              node: {
                type: NodeTypes.ATTRIBUTE,
                name: "test",
              },
            },
          ],
          parent: null,
        },
        context: { ignoredIdentifiers: [] },
        items: [
          {
            type: TemplateTypes.SlotDeclaration,
          },
          {
            type: TemplateTypes.Prop,
            name: "test",
          },
        ],
      });
    });

    it("slot :test='foo'", () => {
      const { result } = parse(`<slot :test='foo'/>`);
      expect(result).toMatchObject({
        slot: {
          type: TemplateTypes.SlotDeclaration,
          node: {
            type: NodeTypes.ELEMENT,
            tagType: ElementTypes.SLOT,
          },
          name: null,
          props: [
            {
              type: TemplateTypes.Prop,
              name: [
                {
                  type: TemplateTypes.Binding,
                  name: "test",
                  ignore: true,
                },
              ],
              value: [
                {
                  type: TemplateTypes.Binding,
                  name: "foo",
                  ignore: false,
                },
              ],
            },
          ],
          parent: null,
        },
        context: { ignoredIdentifiers: [] },
        items: [
          {
            type: TemplateTypes.SlotDeclaration,
          },
          {
            type: TemplateTypes.Prop,
          },
          {
            type: TemplateTypes.Binding,
            name: "test",
          },
          {
            type: TemplateTypes.Binding,
            name: "foo",
          },
        ],
      });
    });
  });

  describe("handleSlotProp", () => {
    function parse(
      content: string,
      context: SlotsContext = { ignoredIdentifiers: [] }
    ) {
      const source = `<template>${content}</template>`;

      const sfc = parseSFC(source, {});

      const template = sfc.descriptor.template;
      const ast = template?.ast!;

      const result = handleSlotProp(ast.children[0] as any, ast, context);

      return {
        source,
        result,
      };
    }

    it("return null if not element", () => {
      const node = {
        type: NodeTypes.COMMENT,
      } as any as ElementNode;
      expect(
        handleSlotProp(node, {} as any, { ignoredIdentifiers: [] })
      ).toBeNull();
    });

    it("return null if no v-slot", () => {
      const { result } = parse("<div></div>");
      expect(result).toBeNull();
    });

    it("v-slot", () => {
      const { result } = parse(`<div v-slot></div>`);

      expect(result).toMatchObject({
        slot: {
          type: TemplateTypes.SlotRender,
          parent: {},
          name: null,
          prop: {
            type: TemplateTypes.Directive,
            name: "slot",
            arg: null,
            exp: null,
            node: {
              type: NodeTypes.DIRECTIVE,
              name: "slot",
              rawName: "v-slot",
            },
          },
        },
        context: { ignoredIdentifiers: [] },
        items: [
          {
            type: TemplateTypes.SlotRender,
          },
        ],
      });
    });

    it("v-slot:name", () => {
      const { result } = parse(`<div v-slot:name></div>`);

      expect(result).toMatchObject({
        slot: {
          type: TemplateTypes.SlotRender,
          parent: {},
          name: [
            {
              type: TemplateTypes.Binding,
              name: "name",
              ignore: true,
            },
          ],
          prop: {
            type: TemplateTypes.Directive,
            name: "slot",
            arg: [
              {
                type: TemplateTypes.Binding,
                name: "name",
                ignore: true,
              },
            ],
            exp: null,
            node: {
              type: NodeTypes.DIRECTIVE,
              name: "slot",
              rawName: "v-slot:name",
            },
          },
        },
        context: { ignoredIdentifiers: [] },
        items: [
          {
            type: TemplateTypes.SlotRender,
          },
        ],
      });
    });

    it("v-slot:[name]", () => {
      const { result } = parse(`<div v-slot:[name]></div>`);

      expect(result).toMatchObject({
        slot: {
          type: TemplateTypes.SlotRender,
          parent: {},
          name: [
            {
              type: TemplateTypes.Binding,
              name: "name",
              ignore: false,
            },
          ],
          prop: {
            type: TemplateTypes.Directive,
            name: "slot",
            arg: [
              {
                type: TemplateTypes.Binding,
                name: "name",
                ignore: false,
              },
            ],
            exp: null,
            node: {
              type: NodeTypes.DIRECTIVE,
              name: "slot",
              rawName: "v-slot:[name]",
            },
          },
        },
        context: { ignoredIdentifiers: [] },
        items: [
          {
            type: TemplateTypes.SlotRender,
          },
          {
            type: TemplateTypes.Binding,
            name: "name",
            ignore: false,
          },
        ],
      });
    });

    it("v-slot='{foo}'", () => {
      const { result } = parse(`<div v-slot='{foo}'></div>`);

      expect(result).toMatchObject({
        slot: {
          type: TemplateTypes.SlotRender,
          parent: {},
          name: null,
          prop: {
            type: TemplateTypes.Directive,
            name: "slot",
            arg: null,
            exp: [
              {
                type: TemplateTypes.Binding,
                name: "foo",
                ignore: false,
              },
            ],
            node: {
              type: NodeTypes.DIRECTIVE,
              name: "slot",
              rawName: "v-slot",
            },
          },
        },
        context: { ignoredIdentifiers: ["foo"] },
        items: [
          {
            type: TemplateTypes.SlotRender,
          },
        ],
      });
    });

    it("v-slot:name='{foo}'", () => {
      const { result } = parse(`<div v-slot:name='{foo}'></div>`);

      expect(result).toMatchObject({
        slot: {
          type: TemplateTypes.SlotRender,
          parent: {},
          name: [
            {
              type: TemplateTypes.Binding,
              name: "name",
              ignore: true,
            },
          ],
          prop: {
            type: TemplateTypes.Directive,
            name: "slot",
            arg: [
              {
                type: TemplateTypes.Binding,
                name: "name",
                ignore: true,
              },
            ],
            exp: [
              {
                type: TemplateTypes.Binding,
                name: "foo",
                ignore: false,
              },
            ],
            node: {
              type: NodeTypes.DIRECTIVE,
              name: "slot",
              rawName: "v-slot:name",
            },
          },
        },
        context: { ignoredIdentifiers: ["foo"] },
        items: [
          {
            type: TemplateTypes.SlotRender,
          },
        ],
      });
    });

    it("v-slot:[name]='{foo}'", () => {
      const { result } = parse(`<div v-slot:[name]='{foo}'></div>`);
      expect(result).toMatchObject({
        slot: {
          type: TemplateTypes.SlotRender,
          parent: {},
          name: [
            {
              type: TemplateTypes.Binding,
              name: "name",
              ignore: false,
            },
          ],
          prop: {
            type: TemplateTypes.Directive,
            name: "slot",
            arg: [
              {
                type: TemplateTypes.Binding,
                name: "name",
                ignore: false,
              },
            ],
            exp: [
              {
                type: TemplateTypes.Binding,
                name: "foo",
                ignore: false,
              },
            ],
            node: {
              type: NodeTypes.DIRECTIVE,
              name: "slot",
              rawName: "v-slot:[name]",
            },
          },
        },
        context: { ignoredIdentifiers: ["foo"] },
        items: [
          {
            type: TemplateTypes.SlotRender,
          },
          {
            type: TemplateTypes.Binding,
            name: "name",
            ignore: false,
          },
        ],
      });
    });

    it("v-slot='{foo:bar}'", () => {
      const { result } = parse(`<div v-slot='{foo:bar}'></div>`);

      expect(result).toMatchObject({
        slot: {
          type: TemplateTypes.SlotRender,
          parent: {},
          name: null,
          prop: {
            type: TemplateTypes.Directive,
            name: "slot",
            arg: null,
            exp: [
              {
                type: TemplateTypes.Binding,
                name: "bar",
                ignore: false,
              },
            ],
            node: {
              type: NodeTypes.DIRECTIVE,
              name: "slot",
              rawName: "v-slot",
            },
          },
        },
        context: { ignoredIdentifiers: ["bar"] },
        items: [
          {
            type: TemplateTypes.SlotRender,
          },
        ],
      });
    });

    it("v-slot:name='{foo:bar}'", () => {
      const { result } = parse(`<div v-slot:name='{foo:bar}'></div>`);

      expect(result).toMatchObject({
        slot: {
          type: TemplateTypes.SlotRender,
          parent: {},
          name: [
            {
              type: TemplateTypes.Binding,
              name: "name",
              ignore: true,
            },
          ],
          prop: {
            type: TemplateTypes.Directive,
            name: "slot",
            arg: [
              {
                type: TemplateTypes.Binding,
                name: "name",
                ignore: true,
              },
            ],
            exp: [
              {
                type: TemplateTypes.Binding,
                name: "bar",
                ignore: false,
              },
            ],
            node: {
              type: NodeTypes.DIRECTIVE,
              name: "slot",
              rawName: "v-slot:name",
            },
          },
        },
        context: { ignoredIdentifiers: ["bar"] },
        items: [
          {
            type: TemplateTypes.SlotRender,
          },
        ],
      });
    });

    it("v-slot:[name]='{foo:bar}'", () => {
      const { result } = parse(`<div v-slot:[name]='{foo:bar}'></div>`);

      expect(result).toMatchObject({
        slot: {
          type: TemplateTypes.SlotRender,
          parent: {},
          name: [
            {
              type: TemplateTypes.Binding,
              name: "name",
              ignore: false,
            },
          ],
          prop: {
            type: TemplateTypes.Directive,
            name: "slot",
            arg: [
              {
                type: TemplateTypes.Binding,
                name: "name",
                ignore: false,
              },
            ],
            exp: [
              {
                type: TemplateTypes.Binding,
                name: "bar",
                ignore: false,
              },
            ],
            node: {
              type: NodeTypes.DIRECTIVE,
              name: "slot",
              rawName: "v-slot:[name]",
            },
          },
        },
        context: { ignoredIdentifiers: ["bar"] },
        items: [
          {
            type: TemplateTypes.SlotRender,
          },
          {
            type: TemplateTypes.Binding,
            name: "name",
            ignore: false,
          },
        ],
      });
    });

    it('v-slot:[`test:${name}`]="{foo:bar}"', () => {
      const { result } = parse(
        `<div v-slot:[\`test:\${name}\`]="{foo:bar}"></div>`
      );

      expect(result).toMatchObject({
        slot: {
          type: TemplateTypes.SlotRender,
          parent: {},
          name: [
            {
              type: TemplateTypes.Binding,
              name: "name",
              ignore: false,
            },
          ],
          prop: {
            type: TemplateTypes.Directive,
            name: "slot",
            arg: [
              {
                type: TemplateTypes.Binding,
                name: "name",
                ignore: false,
              },
            ],
            exp: [
              {
                type: TemplateTypes.Binding,
                name: "bar",
                ignore: false,
              },
            ],
            node: {
              type: NodeTypes.DIRECTIVE,
              name: "slot",
              rawName: "v-slot:[`test:${name}`]",
            },
          },
        },
        context: { ignoredIdentifiers: ["bar"] },
        items: [
          {
            type: TemplateTypes.SlotRender,
          },
          {
            type: TemplateTypes.Binding,
            name: "name",
            ignore: false,
          },
        ],
      });
    });

    describe("sugar", () => {
      it("#name", () => {
        const { result } = parse(`<div #name></div>`);

        expect(result).toMatchObject({
          slot: {
            type: TemplateTypes.SlotRender,
            parent: {},
            name: [
              {
                type: TemplateTypes.Binding,
                name: "name",
                ignore: true,
              },
            ],
            prop: {
              type: TemplateTypes.Directive,
              name: "slot",
              arg: [
                {
                  type: TemplateTypes.Binding,
                  name: "name",
                  ignore: true,
                },
              ],
              exp: null,
              node: {
                type: NodeTypes.DIRECTIVE,
                name: "slot",
                rawName: "#name",
              },
            },
          },
          context: { ignoredIdentifiers: [] },
          items: [
            {
              type: TemplateTypes.SlotRender,
            },
          ],
        });
      });

      it("#[name]", () => {
        const { result } = parse(`<div #[name]></div>`);

        expect(result).toMatchObject({
          slot: {
            type: TemplateTypes.SlotRender,
            parent: {},
            name: [
              {
                type: TemplateTypes.Binding,
                name: "name",
                ignore: false,
              },
            ],
            prop: {
              type: TemplateTypes.Directive,
              name: "slot",
              arg: [
                {
                  type: TemplateTypes.Binding,
                  name: "name",
                  ignore: false,
                },
              ],
              exp: null,
              node: {
                type: NodeTypes.DIRECTIVE,
                name: "slot",
                rawName: "#[name]",
              },
            },
          },
          context: { ignoredIdentifiers: [] },
          items: [
            {
              type: TemplateTypes.SlotRender,
            },
            {
              type: TemplateTypes.Binding,
              name: "name",
              ignore: false,
            },
          ],
        });
      });

      it("#='{foo}'", () => {
        const { result } = parse(`<div #='{foo}'></div>`);

        expect(result).toMatchObject({
          slot: {
            type: TemplateTypes.SlotRender,
            parent: {},
            name: null,
            prop: {
              type: TemplateTypes.Directive,
              name: "slot",
              arg: null,
              exp: [
                {
                  type: TemplateTypes.Binding,
                  name: "foo",
                  ignore: false,
                },
              ],
              node: {
                type: NodeTypes.DIRECTIVE,
                name: "slot",
                rawName: "#",
              },
            },
          },
          context: { ignoredIdentifiers: ["foo"] },
          items: [
            {
              type: TemplateTypes.SlotRender,
            },
          ],
        });
      });

      it("#name='{foo}'", () => {
        const { result } = parse(`<div #name='{foo}'></div>`);

        expect(result).toMatchObject({
          slot: {
            type: TemplateTypes.SlotRender,
            parent: {},
            name: [
              {
                type: TemplateTypes.Binding,
                name: "name",
                ignore: true,
              },
            ],
            prop: {
              type: TemplateTypes.Directive,
              name: "slot",
              arg: [
                {
                  type: TemplateTypes.Binding,
                  name: "name",
                  ignore: true,
                },
              ],
              exp: [
                {
                  type: TemplateTypes.Binding,
                  name: "foo",
                  ignore: false,
                },
              ],
              node: {
                type: NodeTypes.DIRECTIVE,
                name: "slot",
                rawName: "#name",
              },
            },
          },
          context: { ignoredIdentifiers: ["foo"] },
          items: [
            {
              type: TemplateTypes.SlotRender,
            },
          ],
        });
      });

      it("#[name]='{foo}'", () => {
        const { result } = parse(`<div #[name]='{foo}'></div>`);
        expect(result).toMatchObject({
          slot: {
            type: TemplateTypes.SlotRender,
            parent: {},
            name: [
              {
                type: TemplateTypes.Binding,
                name: "name",
                ignore: false,
              },
            ],
            prop: {
              type: TemplateTypes.Directive,
              name: "slot",
              arg: [
                {
                  type: TemplateTypes.Binding,
                  name: "name",
                  ignore: false,
                },
              ],
              exp: [
                {
                  type: TemplateTypes.Binding,
                  name: "foo",
                  ignore: false,
                },
              ],
              node: {
                type: NodeTypes.DIRECTIVE,
                name: "slot",
                rawName: "#[name]",
              },
            },
          },
          context: { ignoredIdentifiers: ["foo"] },
          items: [
            {
              type: TemplateTypes.SlotRender,
            },
            {
              type: TemplateTypes.Binding,
              name: "name",
              ignore: false,
            },
          ],
        });
      });

      it("#='{foo:bar}'", () => {
        const { result } = parse(`<div #='{foo:bar}'></div>`);

        expect(result).toMatchObject({
          slot: {
            type: TemplateTypes.SlotRender,
            parent: {},
            name: null,
            prop: {
              type: TemplateTypes.Directive,
              name: "slot",
              arg: null,
              exp: [
                {
                  type: TemplateTypes.Binding,
                  name: "bar",
                  ignore: false,
                },
              ],
              node: {
                type: NodeTypes.DIRECTIVE,
                name: "slot",
                rawName: "#",
              },
            },
          },
          context: { ignoredIdentifiers: ["bar"] },
          items: [
            {
              type: TemplateTypes.SlotRender,
            },
          ],
        });
      });

      it("#name='{foo:bar}'", () => {
        const { result } = parse(`<div #name='{foo:bar}'></div>`);

        expect(result).toMatchObject({
          slot: {
            type: TemplateTypes.SlotRender,
            parent: {},
            name: [
              {
                type: TemplateTypes.Binding,
                name: "name",
                ignore: true,
              },
            ],
            prop: {
              type: TemplateTypes.Directive,
              name: "slot",
              arg: [
                {
                  type: TemplateTypes.Binding,
                  name: "name",
                  ignore: true,
                },
              ],
              exp: [
                {
                  type: TemplateTypes.Binding,
                  name: "bar",
                  ignore: false,
                },
              ],
              node: {
                type: NodeTypes.DIRECTIVE,
                name: "slot",
                rawName: "#name",
              },
            },
          },
          context: { ignoredIdentifiers: ["bar"] },
          items: [
            {
              type: TemplateTypes.SlotRender,
            },
          ],
        });
      });

      it("#[name]='{foo:bar}'", () => {
        const { result } = parse(`<div #[name]='{foo:bar}'></div>`);

        expect(result).toMatchObject({
          slot: {
            type: TemplateTypes.SlotRender,
            parent: {},
            name: [
              {
                type: TemplateTypes.Binding,
                name: "name",
                ignore: false,
              },
            ],
            prop: {
              type: TemplateTypes.Directive,
              name: "slot",
              arg: [
                {
                  type: TemplateTypes.Binding,
                  name: "name",
                  ignore: false,
                },
              ],
              exp: [
                {
                  type: TemplateTypes.Binding,
                  name: "bar",
                  ignore: false,
                },
              ],
              node: {
                type: NodeTypes.DIRECTIVE,
                name: "slot",
                rawName: "#[name]",
              },
            },
          },
          context: { ignoredIdentifiers: ["bar"] },
          items: [
            {
              type: TemplateTypes.SlotRender,
            },
            {
              type: TemplateTypes.Binding,
              name: "name",
              ignore: false,
            },
          ],
        });
      });

      it('#[`test:${name}`]="{foo:bar}"', () => {
        const { result } = parse(
          `<div #[\`test:\${name}\`]="{foo:bar}"></div>`
        );

        expect(result).toMatchObject({
          slot: {
            type: TemplateTypes.SlotRender,
            parent: {},
            name: [
              {
                type: TemplateTypes.Binding,
                name: "name",
                ignore: false,
              },
            ],
            prop: {
              type: TemplateTypes.Directive,
              name: "slot",
              arg: [
                {
                  type: TemplateTypes.Binding,
                  name: "name",
                  ignore: false,
                },
              ],
              exp: [
                {
                  type: TemplateTypes.Binding,
                  name: "bar",
                  ignore: false,
                },
              ],
              node: {
                type: NodeTypes.DIRECTIVE,
                name: "slot",
                rawName: "#[`test:${name}`]",
              },
            },
          },
          context: { ignoredIdentifiers: ["bar"] },
          items: [
            {
              type: TemplateTypes.SlotRender,
            },
            {
              type: TemplateTypes.Binding,
              name: "name",
              ignore: false,
            },
          ],
        });
      });
    });
  });
});
