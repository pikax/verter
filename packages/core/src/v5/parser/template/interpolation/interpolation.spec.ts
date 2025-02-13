import { InterpolationNode, NodeTypes } from "@vue/compiler-core";
import { handleInterpolation, InterpolationContext } from "./index";
import { parse as parseSFC, compileTemplate } from "@vue/compiler-sfc";
import { TemplateTypes } from "../types";

describe("parser template interpolation", () => {
  function interpolate(
    content: string,
    context: InterpolationContext = { ignoredIdentifiers: [] }
  ) {
    const source = `<template>${content}</template>`;

    const sfc = parseSFC(source, {});

    const template = sfc.descriptor.template;
    const ast = template?.ast!;

    const result = handleInterpolation(ast.children[0] as any, context);

    return {
      source,
      result,
    };
  }

  it("return null if not interpolation", () => {
    const node = {
      type: NodeTypes.ELEMENT,
    } as any as InterpolationNode;

    expect(handleInterpolation(node, { ignoredIdentifiers: [] })).toBeNull();
  });

  it("{{ temp }}", () => {
    const { result, source } = interpolate(`{{ temp }}`);

    expect(result).toMatchObject([
      {
        type: "Interpolation",
      },
      {
        name: "temp",
        ignore: false,
      },
    ]);

    const [_, n] = result as any;

    expect(source.slice(n.node.loc.start.offset, n.node.loc.end.offset)).toBe(
      n.node.loc.source
    );
  });
  it("{{ temp }} but ignored", () => {
    const { result, source } = interpolate(`{{ temp }}`, {
      ignoredIdentifiers: ["temp"],
    });

    expect(result).toMatchObject([
      {
        type: "Interpolation",
      },
      {
        name: "temp",
        ignore: true,
      },
    ]);

    const [_, n] = result as any;

    expect(source.slice(n.node.loc.start.offset, n.node.loc.end.offset)).toBe(
      n.node.loc.source
    );
  });

  it('{{ temp + "temp" }}', () => {
    const { result, source } = interpolate(`{{ temp + "temp" }}`);

    expect(result).toMatchObject([
      {
        type: "Interpolation",
      },
      {
        name: "temp",
        ignore: false,
      },
      {
        type: TemplateTypes.Literal,
        value: "temp",
      },
    ]);

    const [_, n] = result as any;

    expect(source.slice(n.node.loc.start.offset, n.node.loc.end.offset)).toBe(
      n.node.loc.source
    );
  });

  it("{{ temp as string}} ", () => {
    const { result, source } = interpolate(`{{ temp as string }}`);

    expect(result).toMatchObject([
      {
        type: "Interpolation",
      },
      {
        name: "temp",
        ignore: false,
      },
    ]);

    const [_, n] = result as any;

    expect(source.slice(n.node.loc.start.offset, n.node.loc.end.offset)).toBe(
      n.node.loc.source
    );
  });

  it("{{ temp as typeof Foo }} ", () => {
    const { result, source } = interpolate(`{{ temp as typeof Foo }}`);

    expect(result).toMatchObject([
      {
        type: "Interpolation",
      },
      {
        name: "temp",
        ignore: false,
      },
      {
        name: "Foo",
        ignore: false,
      },
    ]);

    // @ts-expect-error
    for (const node of result) {
      expect(
        source.slice(node.node.loc.start.offset, node.node.loc.end.offset)
      ).toBe(node.node.loc.source);
    }
  });

  it("{{ temp as typeof Foo }} ignored", () => {
    const { result, source } = interpolate(`{{ temp as typeof Foo }}`, {
      ignoredIdentifiers: ["temp", "Foo"],
    });

    expect(result).toMatchObject([
      {
        type: "Interpolation",
      },
      {
        name: "temp",
        ignore: true,
      },
      {
        name: "Foo",
        ignore: true,
      },
    ]);

    // @ts-expect-error
    for (const node of result) {
      expect(
        source.slice(node.node.loc.start.offset, node.node.loc.end.offset)
      ).toBe(node.node.loc.source);
    }
  });

  it("{{ temp.toString() }}", () => {
    const { result, source } = interpolate(`{{ temp.toString() }}`);

    expect(result).toMatchObject([
      {
        type: "Interpolation",
      },
      {
        name: "temp",
        ignore: false,
      },
    ]);

    const [_, n] = result as any;

    expect(source.slice(n.node.loc.start.offset, n.node.loc.end.offset)).toBe(
      n.node.loc.source
    );
  });

  it("{{ temp.foo.bar }}", () => {
    const { result, source } = interpolate(`{{ temp.foo.bar }}`);

    expect(result).toMatchObject([
      {
        type: "Interpolation",
      },
      {
        name: "temp",
        ignore: false,
      },
    ]);

    const [_, n] = result as any;

    expect(source.slice(n.node.loc.start.offset, n.node.loc.end.offset)).toBe(
      n.node.loc.source
    );
  });

  it("{{ temp.foo.bar() }}", () => {
    const { result, source } = interpolate(`{{ temp.foo.bar() }}`);

    expect(result).toMatchObject([
      {
        type: "Interpolation",
      },
      {
        name: "temp",
        ignore: false,
      },
    ]);

    const [_, n] = result as any;

    expect(source.slice(n.node.loc.start.offset, n.node.loc.end.offset)).toBe(
      n.node.loc.source
    );
  });

  it("{{ (()=> { temp })() }}", () => {
    const { result, source } = interpolate(`{{ (()=> { temp })() }}`);

    expect(result).toMatchObject([
      {
        type: "Interpolation",
      },

      {
        type: TemplateTypes.Function,
        node: {
          type: "ArrowFunctionExpression",
        },
      },
      {
        name: "temp",
        ignore: false,
      },
    ]);

    const [_, __, n] = result as any;

    expect(source.slice(n.node.loc.start.offset, n.node.loc.end.offset)).toBe(
      n.node.loc.source
    );
  });
  it("{{ (()=> { temp }) }}", () => {
    const { result, source } = interpolate(`{{ (()=> { temp }) }}`);

    expect(result).toMatchObject([
      {
        type: "Interpolation",
      },
      {
        type: TemplateTypes.Function,
        node: {
          type: "ArrowFunctionExpression",
        },
      },
      {
        name: "temp",
        ignore: false,
      },
    ]);

    const [_, __, n] = result as any;

    expect(source.slice(n.node.loc.start.offset, n.node.loc.end.offset)).toBe(
      n.node.loc.source
    );
  });

  it("{{ 'test' }}", () => {
    const { result } = interpolate(`{{ 'test' }}`);

    expect(result).toMatchObject([
      {
        type: "Interpolation",
      },
      {
        type: TemplateTypes.Literal,
        value: "test",
      },
    ]);
  });

  it("{{ [ Foo ] }}", () => {
    const { result, source } = interpolate(`{{ [ Foo ] }}`);

    expect(result).toMatchObject([
      {
        type: "Interpolation",
      },
      {
        name: "Foo",
        ignore: false,
      },
    ]);

    const [_, n] = result as any;

    expect(source.slice(n.node.loc.start.offset, n.node.loc.end.offset)).toBe(
      n.node.loc.source
    );
  });

  it('{{ (Foo["bar"]) }}', () => {
    const { result, source } = interpolate(`{{ (Foo["bar"]) }}`);

    expect(result).toMatchObject([
      {
        type: "Interpolation",
      },
      {
        name: "Foo",
        ignore: false,
      },
      {
        type: TemplateTypes.Literal,
        value: "bar",
      },
    ]);

    const [_, n] = result as any;

    expect(source.slice(n.node.loc.start.offset, n.node.loc.end.offset)).toBe(
      n.node.loc.source
    );
  });

  it("{{ temp | filter }} - binary", () => {
    const { result, source } = interpolate(`{{ temp | filter }}`);

    expect(result).toMatchObject([
      {
        type: "Interpolation",
      },
      {
        name: "temp",
        ignore: false,
      },
      {
        name: "filter",
        ignore: false,
      },
    ]);

    // @ts-expect-error
    for (const node of result) {
      expect(
        source.slice(node.node.loc.start.offset, node.node.loc.end.offset)
      ).toBe(node.node.loc.source);
    }
  });

  it("{{ isActive ? doActive() : doInactive() }}", () => {
    const { result, source } = interpolate(
      `{{ isActive ? doActive() : doInactive() }}`
    );

    expect(result).toMatchObject([
      {
        type: "Interpolation",
      },
      {
        name: "isActive",
        ignore: false,
      },
      {
        name: "doActive",
        ignore: false,
      },
      {
        name: "doInactive",
        ignore: false,
      },
    ]);

    // @ts-expect-error
    for (const node of result) {
      expect(
        source.slice(node.node.loc.start.offset, node.node.loc.end.offset)
      ).toBe(node.node.loc.source);
    }
  });

  it("{{ 'test' }}", () => {
    const { result } = interpolate(`{{ 'test' }}`);

    expect(result).toMatchObject([
      {
        type: TemplateTypes.Interpolation,
      },
      {
        type: TemplateTypes.Literal,
      },
    ]);
  });

  it("{{ temp + foo + }} - invalid input", () => {
    const { result, source } = interpolate(`{{ temp + foo + }}`);

    expect(result).toMatchObject([
      {
        type: "Interpolation",
      },
      {
        name: "temp",
        ignore: false,
      },
      {
        name: "foo",
        ignore: false,
      },
    ]);

    // @ts-expect-error
    for (const node of result) {
      expect(
        source.slice(node.node.loc.start.offset, node.node.loc.end.offset)
      ).toBe(node.node.loc.source);
    }
  });

  it("{{ temp as Foo +  }}", () => {
    const { result, source } = interpolate(`{{ temp as Foo + }}`, {
      ignoredIdentifiers: ["as"],
    });

    expect(result).toMatchObject([
      {
        type: "Interpolation",
      },
      {
        name: "temp",
        ignore: false,
      },
      {
        name: "as",
        ignore: true,
      },
      {
        name: "Foo",
        ignore: false,
      },
    ]);

    // @ts-expect-error
    for (const node of result) {
      expect(
        source.slice(node.node.loc.start.offset, node.node.loc.end.offset)
      ).toBe(node.node.loc.source);
    }
  });

  it("{{ foo(baz + 1, { key: kuz }) }}", () => {
    const { result, source } = interpolate(`{{ foo(baz + 1, { key: kuz }) }}`);

    expect(result).toMatchObject([
      {
        type: "Interpolation",
      },
      {
        name: "foo",
        ignore: false,
      },
      {
        name: "baz",
        ignore: false,
      },
      {
        type: TemplateTypes.Literal,
        value: 1,
      },
      {
        name: "kuz",
        ignore: false,
      },
    ]);

    // @ts-expect-error
    for (const node of result) {
      expect(
        source.slice(node.node.loc.start.offset, node.node.loc.end.offset)
      ).toBe(node.node.loc.source);
    }
  });

  it("should ignore params arrow function", () => {
    const { result, source } = interpolate(
      `{{ ((ar)=> { ar.toString()})(1) }}`
    );

    expect(result).toMatchObject([
      {
        type: "Interpolation",
      },
      {
        type: TemplateTypes.Function,
        node: {
          type: "ArrowFunctionExpression",
        },
      },
      {
        name: "ar",
        ignore: true,
      },
      {
        name: "ar",
        ignore: true,
      },
      {
        type: TemplateTypes.Literal,
        value: 1,
      },
    ]);

    // @ts-expect-error
    for (const node of result) {
      expect(
        source.slice(node.node.loc.start.offset, node.node.loc.end.offset)
      ).toBe(node.node.loc.source);
    }
  });
  it("should ignore params function", () => {
    const { result, source } = interpolate(
      `{{ (function (ar) { ar.toString()})(1) }}`
    );

    expect(result).toMatchObject([
      {
        type: "Interpolation",
      },
      {
        type: TemplateTypes.Function,
        node: {
          type: "FunctionExpression",
        },
      },
      {
        name: "ar",
        ignore: true,
      },
      {
        name: "ar",
        ignore: true,
      },
      {
        type: TemplateTypes.Literal,
        value: 1,
      },
    ]);

    // @ts-expect-error
    for (const node of result) {
      expect(
        source.slice(node.node.loc.start.offset, node.node.loc.end.offset)
      ).toBe(node.node.loc.source);
    }
  });
  it("should ignore params function named", () => {
    const { result, source } = interpolate(
      `{{ (function foo(ar) { ar.toString(); foo();})(1) }}`
    );

    expect(result).toMatchObject([
      {
        type: "Interpolation",
      },
      {
        type: TemplateTypes.Function,
        node: {
          type: "FunctionExpression",
        },
      },
      {
        name: "foo",
        ignore: true,
      },
      {
        name: "ar",
        ignore: true,
      },
      {
        name: "ar",
        ignore: true,
      },
      {
        name: "foo",
        ignore: true,
      },
      {
        type: TemplateTypes.Literal,
        value: 1,
      },
    ]);

    // @ts-expect-error
    for (const node of result) {
      expect(
        source.slice(node.node.loc.start.offset, node.node.loc.end.offset)
      ).toBe(node.node.loc.source);
    }
  });

  it("ignore class", () => {
    const { result, source } = interpolate(
      `{{ (class foo { constructor() { this.toString(); new foo(); } }) }}`
    );

    expect(result).toMatchObject([
      {
        type: "Interpolation",
      },
      {
        name: "foo",
        ignore: true,
      },

      {
        name: "foo",
        ignore: true,
      },
    ]);

    // @ts-expect-error
    for (const node of result) {
      expect(
        source.slice(node.node.loc.start.offset, node.node.loc.end.offset)
      ).toBe(node.node.loc.source);
    }
  });
});
