import { parse as parseSFC } from "@vue/compiler-sfc";
import { ElementContext, handleElement } from "./index.js";
import { TemplateTypes } from "../types.js";
import element from "../../../../plugins/template/v2/transpile/transpilers/element/element.js";

describe("parser template element", () => {
  function parse(
    content: string,
    context: ElementContext = {
      ignoredIdentifiers: [],
      conditions: [],
      inFor: false,
    }
  ) {
    const source = `<template>${content}</template>`;

    const sfc = parseSFC(source, {});

    const template = sfc.descriptor.template;
    const ast = template?.ast!;

    const result = handleElement(ast.children[0] as any, ast, context);

    return {
      source,
      result,
    };
  }

  it("<div>", () => {
    const { result } = parse(`<div></div>`);

    expect(result).toMatchObject({
      element: {
        type: "Element",
        tag: "div",
        node: {
          tag: "div",
        },
        parent: {},

        props: [],

        ref: null,
        loop: null,
        condition: null,
      },

      items: [
        {
          type: "Element",
          tag: "div",
        },
      ],
    });
  });

  it('div v-if="true"', () => {
    const { result } = parse(`<div v-if="true"></div>`);

    expect(result).toMatchObject({
      element: {
        type: "Element",
        tag: "div",
        node: {
          tag: "div",
        },
        parent: {},

        props: [],

        ref: null,
        loop: null,
        condition: {
          type: TemplateTypes.Condition,
          node: {
            name: "if",
          },
        },
      },

      items: [
        {
          type: TemplateTypes.Condition,
          node: {
            name: "if",
          },
        },
        {
          type: TemplateTypes.Binding,
          name: "true",
        },
        {
          type: TemplateTypes.Element,
          tag: "div",
        },
      ],
    });
  });

  it('div v-if="temp" :temp="temp"', () => {
    const { result } = parse(`<div v-if="temp" :temp="temp"></div>`);

    expect(result).toMatchObject({
      element: {
        type: "Element",
        tag: "div",
        node: {
          tag: "div",
        },
        parent: {},

        props: [
          {
            type: TemplateTypes.Prop,
            arg: [{ name: "temp" }],
          },
        ],

        ref: null,
        loop: null,
        condition: {
          type: TemplateTypes.Condition,
          node: {
            name: "if",
          },
        },
      },

      items: [
        {
          type: TemplateTypes.Condition,
        },
        {
          type: TemplateTypes.Binding,
          name: "temp",
          ignore: false,
        },
        {
          type: TemplateTypes.Element,
          tag: "div",
        },
        {
          type: TemplateTypes.Prop,
          arg: [{ name: "temp" }],
        },
        {
          type: TemplateTypes.Binding,
          name: "temp",
          ignore: true,
        },
        {
          type: TemplateTypes.Binding,
          name: "temp",
          ignore: false,
        },
      ],
    });
  });

  it('div v-for="item in items" :temp="temp"', () => {
    const { result } = parse(`<div v-for="item in items" :temp="item"></div>`);

    expect(result).toMatchObject({
      element: {
        type: "Element",
        tag: "div",
        node: {
          tag: "div",
        },
        parent: {},

        props: [
          {
            type: TemplateTypes.Prop,
            arg: [{ name: "temp" }],
          },
        ],

        ref: null,
        loop: {
          type: TemplateTypes.Loop,
          node: {
            name: "for",
          },
        },
        condition: null,
      },

      items: [
        {
          type: TemplateTypes.Loop,
          node: {
            name: "for",
          },
        },
        {
          type: TemplateTypes.Binding,
          name: "items",
          ignore: false,
        },
        {
          type: TemplateTypes.Element,
          tag: "div",
        },
        {
          type: TemplateTypes.Prop,
          arg: [{ name: "temp" }],
        },
        {
          type: TemplateTypes.Binding,
          name: "temp",
          ignore: true,
        },
        {
          type: TemplateTypes.Binding,
          name: "item",
          ignore: true,
        },
      ],
    });
  });

  it('div v-if="temp" v-for="temp in items" :temp="temp"', () => {
    const { result } = parse(
      `<div v-if="temp" v-for="temp in items" :temp="temp"></div>`
    );

    expect(result).toMatchObject({
      element: {
        type: "Element",
        tag: "div",
        node: {
          tag: "div",
        },
        parent: {},

        props: [
          {
            type: TemplateTypes.Prop,
            arg: [{ name: "temp" }],
          },
        ],

        ref: null,
        loop: {
          type: TemplateTypes.Loop,
          node: {
            name: "for",
          },
        },
        condition: {
          type: TemplateTypes.Condition,
          node: {
            name: "if",
          },
        },
      },

      items: [
        {
          type: TemplateTypes.Condition,
          node: {
            name: "if",
          },
        },
        {
          type: TemplateTypes.Binding,
          name: "temp",
          ignore: false,
        },
        {
          type: TemplateTypes.Loop,
          node: {
            name: "for",
          },
        },
        {
          type: TemplateTypes.Binding,
          name: "items",
          ignore: false,
        },
        {
          type: TemplateTypes.Element,
          tag: "div",
        },
        {
          type: TemplateTypes.Prop,
          arg: [{ name: "temp" }],
        },
        {
          type: TemplateTypes.Binding,
          name: "temp",
          ignore: true,
        },
        {
          type: TemplateTypes.Binding,
          name: "temp",
          ignore: true,
        },
      ],
    });
  });

  it('div :temp="temp" v-for="temp in items" v-if="temp" ', () => {
    const { result } = parse(
      `<div v-if="temp" v-for="temp in items" :temp="temp"></div>`
    );

    expect(result).toMatchObject({
      element: {
        type: "Element",
        tag: "div",
        node: {
          tag: "div",
        },
        parent: {},

        props: [
          {
            type: TemplateTypes.Prop,
            arg: [{ name: "temp" }],
          },
        ],

        ref: null,
        loop: {
          type: TemplateTypes.Loop,
          node: {
            name: "for",
          },
        },
        condition: {
          type: TemplateTypes.Condition,
          node: {
            name: "if",
          },
        },
      },

      items: [
        {
          type: TemplateTypes.Condition,
          node: {
            name: "if",
          },
        },
        {
          type: TemplateTypes.Binding,
          name: "temp",
          ignore: false,
        },
        {
          type: TemplateTypes.Loop,
          node: {
            name: "for",
          },
        },
        {
          type: TemplateTypes.Binding,
          name: "items",
          ignore: false,
        },
        {
          type: TemplateTypes.Element,
          tag: "div",
        },
        {
          type: TemplateTypes.Prop,
          arg: [{ name: "temp" }],
        },
        {
          type: TemplateTypes.Binding,
          name: "temp",
          ignore: true,
        },
        {
          type: TemplateTypes.Binding,
          name: "temp",
          ignore: true,
        },
      ],
    });
  });

  it('v-slot="slotProps"', () => {
    const { result } = parse(
      `<div v-slot="slotProps" :name="slotProps.test">{{slotProps}}</div>`
    );
    expect(result).toMatchObject({
      element: {
        context: {
          ignoredIdentifiers: ["slotProps"],
        },
      },
    });
  });

  describe("ref", () => {
    it('ref="name"', () => {
      const { result } = parse(`<div ref="name"></div>`);
      expect(result).toMatchObject({
        element: {
          ref: {
            type: TemplateTypes.Prop,
            name: "ref",
            value: "name",
          },
        },
      });
    });

    it(':ref="name"', () => {
      const { result } = parse(`<div :ref="name"></div>`);
      expect(result).toMatchObject({
        element: {
          ref: {
            type: TemplateTypes.Prop,
            name: "bind",
            arg: [{ name: "ref" }],
            exp: [{ name: "name" }],
          },
        },
      });
    });

    it(':ref="el=> name = el"', () => {
      const { result } = parse(`<div :ref="(el)=> name = el"></div>`);
      expect(result).toMatchObject({
        element: {
          ref: {
            type: TemplateTypes.Prop,
            name: "bind",
            arg: [{ name: "ref" }],
            node: {
              exp: {
                ast: {
                  type: "ArrowFunctionExpression",
                },
              },
            },
          },
        },
      });
    });
  });
});
