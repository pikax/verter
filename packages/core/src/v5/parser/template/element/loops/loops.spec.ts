import { parse as parseSFC } from "@vue/compiler-sfc";
import { LoopsContext, handleLoopProp } from "./loops";
import { NodeTypes } from "@vue/compiler-core";
import { TemplateTypes } from "../../types.js";

describe("parser template element loops", () => {
  function parse(
    content: string,
    context: LoopsContext = { ignoredIdentifiers: [], inFor: false }
  ) {
    const source = `<template>${content}</template>`;

    const sfc = parseSFC(source, {});

    const template = sfc.descriptor.template;
    const ast = template?.ast!;

    const result = handleLoopProp(ast.children[0] as any, ast, context);

    return {
      source,
      result,
    };
  }

  it("v-for='item in items'", () => {
    const content = `<div v-for='item in items'></div>`;
    const { result } = parse(content);

    expect(result).toMatchObject({
      loop: {
        type: TemplateTypes.Loop,
        node: {
          type: NodeTypes.DIRECTIVE,
          name: "for",
          forParseResult: {
            source: {},
            value: {},
          },
        },

        element: {
          type: NodeTypes.ELEMENT,
          tag: "div",
        },
        parent: {},
      },
      items: [
        {
          type: TemplateTypes.Loop,
        },
        {
          type: TemplateTypes.Binding,
          name: "items",
          ignore: false,
        },
      ],
      context: {
        ignoredIdentifiers: ["item"],
      },
    });
  });

  it('v-for="{ message } in items"', () => {
    const content = `<div v-for="{ message } in items"></div>`;
    const { result } = parse(content);

    expect(result).toMatchObject({
      loop: {
        type: TemplateTypes.Loop,
        node: {
          type: NodeTypes.DIRECTIVE,
          name: "for",
          forParseResult: {
            source: {},
            value: {},
          },
        },

        element: {
          type: NodeTypes.ELEMENT,
          tag: "div",
        },
        parent: {},
      },
      items: [
        {
          type: TemplateTypes.Loop,
        },
        {
          type: TemplateTypes.Binding,
          name: "items",
          ignore: false,
        },
      ],
      context: {
        ignoredIdentifiers: ["message"],
      },
    });
  });

  it('v-for="(item, index) in items">', () => {
    const content = `<div v-for="(item, index) in items"></div>`;
    const { result } = parse(content);

    expect(result).toMatchObject({
      loop: {
        type: TemplateTypes.Loop,
        node: {
          type: NodeTypes.DIRECTIVE,
          name: "for",
          forParseResult: {
            source: {},
            value: {},
          },
        },

        element: {
          type: NodeTypes.ELEMENT,
          tag: "div",
        },
        parent: {},
      },
      items: [
        {
          type: TemplateTypes.Loop,
        },
        {
          type: TemplateTypes.Binding,
          name: "items",
          ignore: false,
        },
      ],
      context: {
        ignoredIdentifiers: ["item", "index"],
      },
    });
  });
  it('v-for="({ message }, index) in items"', () => {
    const content = `<div v-for="({ message }, index) in items"></div>`;
    const { result } = parse(content);

    expect(result).toMatchObject({
      loop: {
        type: TemplateTypes.Loop,
        node: {
          type: NodeTypes.DIRECTIVE,
          name: "for",
          forParseResult: {
            source: {},
            value: {},
          },
        },

        element: {
          type: NodeTypes.ELEMENT,
          tag: "div",
        },
        parent: {},
      },
      items: [
        {
          type: TemplateTypes.Loop,
        },
        {
          type: TemplateTypes.Binding,
          name: "items",
          ignore: false,
        },
      ],
      context: {
        ignoredIdentifiers: ["message", "index"],
      },
    });
  });
  it('v-for="item of items"', () => {
    const content = `<div v-for="item of items"></div>`;
    const { result } = parse(content);

    expect(result).toMatchObject({
      loop: {
        type: TemplateTypes.Loop,
        node: {
          type: NodeTypes.DIRECTIVE,
          name: "for",
          forParseResult: {
            source: {},
            value: {},
          },
        },

        element: {
          type: NodeTypes.ELEMENT,
          tag: "div",
        },
        parent: {},
      },
      items: [
        {
          type: TemplateTypes.Loop,
        },
        {
          type: TemplateTypes.Binding,
          name: "items",
          ignore: false,
        },
      ],
      context: {
        ignoredIdentifiers: ["item"],
      },
    });
  });
  it('v-for="item   of items"', () => {
    const content = `<div v-for="item   of items"></div>`;
    const { result } = parse(content);

    expect(result).toMatchObject({
      loop: {
        type: TemplateTypes.Loop,
        node: {
          type: NodeTypes.DIRECTIVE,
          name: "for",
          forParseResult: {
            source: {},
            value: {},
          },
        },

        element: {
          type: NodeTypes.ELEMENT,
          tag: "div",
        },
        parent: {},
      },
      items: [
        {
          type: TemplateTypes.Loop,
        },
        {
          type: TemplateTypes.Binding,
          name: "items",
          ignore: false,
        },
      ],
      context: {
        ignoredIdentifiers: ["item"],
      },
    });
  });
  it('v-for="item   of     items"', () => {
    const content = `<div v-for="item   of     items"></div>`;
    const { result } = parse(content);

    expect(result).toMatchObject({
      loop: {
        type: TemplateTypes.Loop,
      },
      items: [
        {
          type: TemplateTypes.Loop,
        },
        {
          type: TemplateTypes.Binding,
          name: "items",
          ignore: false,
        },
      ],
      context: {
        ignoredIdentifiers: ["item"],
      },
    });
  });
  it('v-for="value in myObject"', () => {
    const content = `<div v-for="value in myObject"></div>`;
    const { result } = parse(content);

    expect(result).toMatchObject({
      loop: {
        type: TemplateTypes.Loop,
      },
      items: [
        {
          type: TemplateTypes.Loop,
        },
        {
          type: TemplateTypes.Binding,
          name: "myObject",
          ignore: false,
        },
      ],
      context: {
        ignoredIdentifiers: ["value"],
      },
    });
  });
  it('v-for="(value, key) in myObject"', () => {
    const content = `<div v-for="(value, key) in myObject"></div>`;
    const { result } = parse(content);

    expect(result).toMatchObject({
      loop: {
        type: TemplateTypes.Loop,
      },
      items: [
        {
          type: TemplateTypes.Loop,
        },
        {
          type: TemplateTypes.Binding,
          name: "myObject",
          ignore: false,
        },
      ],
      context: {
        ignoredIdentifiers: ["value", "key"],
      },
    });
  });
  it('v-for="(value, key, index) in myObject"', () => {
    const content = `<div v-for="(value, key, index) in myObject"></div>`;
    const { result } = parse(content);

    expect(result).toMatchObject({
      loop: {
        type: TemplateTypes.Loop,
      },
      items: [
        {
          type: TemplateTypes.Loop,
        },
        {
          type: TemplateTypes.Binding,
          name: "myObject",
          ignore: false,
        },
      ],
      context: {
        ignoredIdentifiers: ["value", "key", "index"],
      },
    });
  });
  it('v-for="n in 10"', () => {
    const content = `<div v-for="n in 10"></div>`;
    const { result } = parse(content);

    expect(result).toMatchObject({
      loop: {
        type: TemplateTypes.Loop,
      },
      items: [
        {
          type: TemplateTypes.Loop,
        },
        {
          type: TemplateTypes.Literal,
          content: "10",
          value: 10,
        },
      ],
      context: {
        ignoredIdentifiers: ["n"],
      },
    });
  });

  it('v-for="n in [1, 42]"', () => {
    const content = `<div v-for="n in [1, 42]"></div>`;
    const { result } = parse(content);

    expect(result).toMatchObject({
      loop: {
        type: TemplateTypes.Loop,
      },
      items: [
        {
          type: TemplateTypes.Loop,
        },
        {
          type: TemplateTypes.Literal,
          content: "1",
          value: 1,
        },
        {
          type: TemplateTypes.Literal,
          content: "42",
          value: 42,
        },
      ],
      context: {
        ignoredIdentifiers: ["n"],
      },
    });
  });
});
