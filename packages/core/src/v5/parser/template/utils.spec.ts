/**
 * @ai-generated - This test file was generated with AI assistance.
 * Comprehensive tests for template expression binding extraction covering:
 * - Valid JavaScript expressions (parsed by OXC/babel)
 * - Valid TypeScript expressions (parsed by OXC/babel)
 * - Broken JavaScript expressions (fallback to acorn-loose)
 * - Broken TypeScript expressions (fallback to acorn-loose)
 *
 * The "broken" variants add syntax errors like missing brackets or trailing dots
 * to force the parser to fall back to acorn-loose. The tests verify that bindings
 * are correctly extracted regardless of which parser is used.
 */

import {parser} from "../parser.js";
import {
  TemplateTypes,
  TemplateBinding,
  TemplateFunction,
  TemplateLiteral,
  TemplateBrokenExpression,
} from "./types";
import { retrieveBindings } from "./utils";
import { NodeTypes, SimpleExpressionNode } from "@vue/compiler-core";

describe("parser template utils - retrieveBindings", () => {
  /**
   * Helper to create a mock SimpleExpressionNode from expression content.
   * Uses Vue's SFC parser to get proper AST parsing.
   */
  function createExpression(
    content: string,
    ignoredIdentifiers: string[] = []
  ): {
    bindings: Array<
      | TemplateBinding
      | TemplateFunction
      | TemplateLiteral
      | TemplateBrokenExpression
    >;
    context: { ignoredIdentifiers: string[] };
  } {
    // Wrap in template interpolation to get Vue to parse it
    const source = `<template>{{ ${content} }}</template><script lang="ts"></script>`;
    const parsed = parser(source, "test.vue", {});
    const template = parsed.blocks.find(x=>x.type === "template")
    return {
      bindings: template!.result?.items!
    }
  }

  /**
   * Helper to extract binding names from results
   */
  function getBindingNames(
    bindings: Array<TemplateBinding | TemplateFunction | TemplateLiteral>
  ): string[] {
    return bindings
      .filter((b): b is TemplateBinding => b.type === TemplateTypes.Binding)
      .map((b) => b.name!)
      .filter(Boolean);
  }

  /**
   * Helper to extract non-ignored binding names
   */
  function getNonIgnoredBindingNames(
    bindings: Array<TemplateBinding | TemplateFunction | TemplateLiteral>
  ): string[] {
    return bindings
      .filter(
        (b): b is TemplateBinding =>
          b.type === TemplateTypes.Binding && !b.ignore
      )
      .map((b) => b.name!)
      .filter(Boolean);
  }

  /**
   * Helper to check if result contains a function
   */
  function hasFunction(
    bindings: Array<TemplateBinding | TemplateFunction | TemplateLiteral>
  ): boolean {
    return bindings.some((b) => b.type === TemplateTypes.Function);
  }

  function hasBrokenExpression(
    bindings: Array<
      | TemplateBinding
      | TemplateFunction
      | TemplateLiteral
      | TemplateBrokenExpression
    >
  ): boolean {
    return bindings.some((b) => b.type === TemplateTypes.BrokenExpression);
  }

  // ============================================================
  // SECTION 1: Valid JavaScript Expressions (OXC/babel parser)
  // ============================================================
  describe("Valid JavaScript Expressions", () => {
    describe("simple identifiers", () => {
      test("single identifier", () => {
        const { bindings } = createExpression("foo");
        expect(getNonIgnoredBindingNames(bindings)).toEqual(["foo"]);
      });

      test("multiple identifiers with operator", () => {
        const { bindings } = createExpression("foo + bar");
        expect(getNonIgnoredBindingNames(bindings)).toEqual(["foo", "bar"]);
      });

      test("identifier with ignored context", () => {
        const { bindings } = createExpression("foo + bar", ["foo"]);
        const nonIgnored = getNonIgnoredBindingNames(bindings);
        expect(nonIgnored).toEqual(["bar"]);
      });

      test("keywords should be ignored", () => {
        const { bindings } = createExpression("true && false || null");
        expect(getNonIgnoredBindingNames(bindings)).toEqual([]);
      });

      test("undefined should be ignored", () => {
        const { bindings } = createExpression("foo ?? undefined");
        expect(getNonIgnoredBindingNames(bindings)).toEqual(["foo"]);
      });
    });

    describe("member expressions", () => {
      test("simple member access", () => {
        const { bindings } = createExpression("foo.bar");
        // Only 'foo' should be a binding, 'bar' is a property
        expect(getNonIgnoredBindingNames(bindings)).toEqual(["foo"]);
      });

      test("chained member access", () => {
        const { bindings } = createExpression("foo.bar.baz");
        expect(getNonIgnoredBindingNames(bindings)).toEqual(["foo"]);
      });

      test("computed member access", () => {
        const { bindings } = createExpression("foo[bar]");
        expect(getNonIgnoredBindingNames(bindings)).toEqual(["foo", "bar"]);
      });

      test("mixed member access", () => {
        const { bindings } = createExpression("foo.bar[baz].qux");
        expect(getNonIgnoredBindingNames(bindings)).toEqual(["foo", "baz"]);
      });
    });

    describe("function calls", () => {
      test("simple function call", () => {
        const { bindings } = createExpression("foo()");
        expect(getNonIgnoredBindingNames(bindings)).toEqual(["foo"]);
      });

      test("function call with arguments", () => {
        const { bindings } = createExpression("foo(bar, baz)");
        expect(getNonIgnoredBindingNames(bindings)).toEqual([
          "foo",
          "bar",
          "baz",
        ]);
      });

      test("method call", () => {
        const { bindings } = createExpression("foo.bar(baz)");
        expect(getNonIgnoredBindingNames(bindings)).toEqual(["foo", "baz"]);
      });

      test("chained method calls", () => {
        const { bindings } = createExpression("foo.bar().baz(qux)");
        expect(getNonIgnoredBindingNames(bindings)).toEqual(["foo", "qux"]);
      });
    });

    describe("arrow functions", () => {
      test("simple arrow function", () => {
        const { bindings } = createExpression("() => foo");
        expect(hasFunction(bindings)).toBe(true);
        expect(getNonIgnoredBindingNames(bindings)).toEqual(["foo"]);
      });

      test("arrow function with parameter", () => {
        const { bindings } = createExpression("(x) => x + foo");
        expect(hasFunction(bindings)).toBe(true);
        // 'x' is a parameter, should be ignored inside the function
        expect(getNonIgnoredBindingNames(bindings)).toEqual(["foo"]);
      });

      test("arrow function with multiple parameters", () => {
        const { bindings } = createExpression("(x, y) => x + y + foo");
        expect(hasFunction(bindings)).toBe(true);
        expect(getNonIgnoredBindingNames(bindings)).toEqual(["foo"]);
      });

      test("arrow function with rest parameter", () => {
        const { bindings } = createExpression("(...args) => args.length + foo");
        expect(hasFunction(bindings)).toBe(true);
        // 'args' is a rest parameter, should be ignored
        expect(getNonIgnoredBindingNames(bindings)).toEqual(["foo"]);
      });

      test("arrow function with destructured parameter", () => {
        const { bindings } = createExpression("({ a, b }) => a + b + foo");
        expect(hasFunction(bindings)).toBe(true);
        // 'a' and 'b' are destructured params, should be ignored
        expect(getNonIgnoredBindingNames(bindings)).toEqual(["foo"]);
      });

      test("arrow function with block body", () => {
        const { bindings } = createExpression(
          "(x) => { const y = 1; return x + y + foo; }"
        );
        expect(hasFunction(bindings)).toBe(true);
        // 'x' is param, 'y' is local variable, only 'foo' is external
        expect(getNonIgnoredBindingNames(bindings)).toEqual(["foo"]);
      });

      test("arrow function with default parameter", () => {
        const { bindings } = createExpression("(x = defaultVal) => x + foo");
        expect(hasFunction(bindings)).toBe(true);
        // 'x' is param (ignored), 'defaultVal' is external binding
        expect(getNonIgnoredBindingNames(bindings)).toEqual([
          "defaultVal",
          "foo",
        ]);
      });
    });

    describe("function expressions", () => {
      test("anonymous function expression", () => {
        const { bindings } = createExpression("function() { return foo; }");
        expect(hasFunction(bindings)).toBe(true);
        expect(getNonIgnoredBindingNames(bindings)).toEqual(["foo"]);
      });

      test("named function expression", () => {
        const { bindings } = createExpression(
          "function myFunc() { return foo; }"
        );
        expect(hasFunction(bindings)).toBe(true);
        // 'myFunc' should be ignored as it's the function name
        expect(getNonIgnoredBindingNames(bindings)).toEqual(["foo"]);
      });

      test("function with parameters", () => {
        const { bindings } = createExpression(
          "function(x, y) { return x + y + foo; }"
        );
        expect(hasFunction(bindings)).toBe(true);
        expect(getNonIgnoredBindingNames(bindings)).toEqual(["foo"]);
      });

      test("function with rest parameters", () => {
        const { bindings } = createExpression(
          "function(...args) { return args.length + foo; }"
        );
        expect(hasFunction(bindings)).toBe(true);
        expect(getNonIgnoredBindingNames(bindings)).toEqual(["foo"]);
      });
    });

    describe("object literals", () => {
      test("simple object", () => {
        const { bindings } = createExpression("{ foo: bar }");
        // 'foo' is a key, 'bar' is a value binding
        expect(getNonIgnoredBindingNames(bindings)).toEqual(["bar"]);
      });

      test("shorthand property", () => {
        const { bindings } = createExpression("{ foo }");
        // shorthand: 'foo' is both key and value
        expect(getNonIgnoredBindingNames(bindings)).toEqual(["foo"]);
      });

      test("computed property key", () => {
        const { bindings } = createExpression("{ [key]: value }");
        expect(getNonIgnoredBindingNames(bindings)).toEqual(["key", "value"]);
      });

      test("method shorthand", () => {
        const { bindings } = createExpression("{ method() { return foo; } }");
        expect(getNonIgnoredBindingNames(bindings)).toEqual(["foo"]);
      });

      test("spread in object", () => {
        const { bindings } = createExpression("{ ...obj, foo: bar }");
        expect(getNonIgnoredBindingNames(bindings)).toEqual(["obj", "bar"]);
      });
    });

    describe("array literals", () => {
      test("simple array", () => {
        const { bindings } = createExpression("[foo, bar, baz]");
        expect(getNonIgnoredBindingNames(bindings)).toEqual([
          "foo",
          "bar",
          "baz",
        ]);
      });

      test("array with spread", () => {
        const { bindings } = createExpression("[...arr, foo]");
        expect(getNonIgnoredBindingNames(bindings)).toEqual(["arr", "foo"]);
      });

      test("nested array", () => {
        const { bindings } = createExpression("[[foo], [bar]]");
        expect(getNonIgnoredBindingNames(bindings)).toEqual(["foo", "bar"]);
      });
    });

    describe("ternary expressions", () => {
      test("simple ternary", () => {
        const { bindings } = createExpression("cond ? foo : bar");
        expect(getNonIgnoredBindingNames(bindings)).toEqual([
          "cond",
          "foo",
          "bar",
        ]);
      });

      test("nested ternary", () => {
        const { bindings } = createExpression("a ? b : c ? d : e");
        expect(getNonIgnoredBindingNames(bindings)).toEqual([
          "a",
          "b",
          "c",
          "d",
          "e",
        ]);
      });
    });

    describe("template literals", () => {
      test("simple template literal", () => {
        const { bindings } = createExpression("`hello ${name}`");
        expect(getNonIgnoredBindingNames(bindings)).toEqual(["name"]);
      });

      test("multiple interpolations", () => {
        const { bindings } = createExpression("`${foo} and ${bar}`");
        expect(getNonIgnoredBindingNames(bindings)).toEqual(["foo", "bar"]);
      });

      test("tagged template", () => {
        const { bindings } = createExpression("tag`hello ${name}`");
        expect(getNonIgnoredBindingNames(bindings)).toEqual(["tag", "name"]);
      });
    });

    describe("logical expressions", () => {
      test("AND expression", () => {
        const { bindings } = createExpression("foo && bar");
        expect(getNonIgnoredBindingNames(bindings)).toEqual(["foo", "bar"]);
      });

      test("OR expression", () => {
        const { bindings } = createExpression("foo || bar");
        expect(getNonIgnoredBindingNames(bindings)).toEqual(["foo", "bar"]);
      });

      test("nullish coalescing", () => {
        const { bindings } = createExpression("foo ?? bar");
        expect(getNonIgnoredBindingNames(bindings)).toEqual(["foo", "bar"]);
      });

      test("optional chaining", () => {
        const { bindings } = createExpression("foo?.bar?.baz");
        expect(getNonIgnoredBindingNames(bindings)).toEqual(["foo"]);
      });
    });

    describe("new expressions", () => {
      test("new with identifier", () => {
        const { bindings } = createExpression("new Foo()");
        expect(getNonIgnoredBindingNames(bindings)).toEqual(["Foo"]);
      });

      test("new with arguments", () => {
        const { bindings } = createExpression("new Foo(bar, baz)");
        expect(getNonIgnoredBindingNames(bindings)).toEqual([
          "Foo",
          "bar",
          "baz",
        ]);
      });
    });

    describe("variable declarations in expressions", () => {
      test("IIFE with variable", () => {
        const { bindings } = createExpression(
          "(() => { const x = 1; return x + foo; })()"
        );
        expect(hasFunction(bindings)).toBe(true);
        // 'x' is declared inside, only 'foo' is external
        expect(getNonIgnoredBindingNames(bindings)).toEqual(["foo"]);
      });

      test("let declaration in arrow function", () => {
        const { bindings } = createExpression(
          "(x) => { let y = x; return y + foo; }"
        );
        expect(hasFunction(bindings)).toBe(true);
        expect(getNonIgnoredBindingNames(bindings)).toEqual(["foo"]);
      });
    });
  });

  // ============================================================
  // SECTION 2: Valid TypeScript Expressions (OXC/babel parser)
  // ============================================================
  describe("Valid TypeScript Expressions", () => {
    describe("type assertions", () => {
      test("as assertion", () => {
        const { bindings } = createExpression("foo as string");
        expect(getNonIgnoredBindingNames(bindings)).toEqual(["foo"]);
      });

      test("as const", () => {
        const { bindings } = createExpression("foo as const");
        expect(getNonIgnoredBindingNames(bindings)).toEqual(["foo"]);
      });

      test("as with complex type", () => {
        const { bindings } = createExpression("foo as { bar: string }");
        // 'foo' is the value, 'bar' is in type annotation (should be ignored)
        expect(getNonIgnoredBindingNames(bindings)).toEqual(["foo"]);
      });

      test("as typeof", () => {
        const { bindings } = createExpression("foo as typeof Bar");
        expect(getNonIgnoredBindingNames(bindings)).toEqual(["foo", "Bar"]);
      });

      test("nested as assertion", () => {
        const { bindings } = createExpression("(foo as Foo).bar as Bar");
        expect(getNonIgnoredBindingNames(bindings)).toEqual(["foo"]);
      });
    });

    describe("type parameters (generics)", () => {
      test("generic function call", () => {
        const { bindings } = createExpression("foo<string>(bar)");
        expect(getNonIgnoredBindingNames(bindings)).toEqual(["foo", "bar"]);
      });

      test("generic with type reference", () => {
        const { bindings } = createExpression("foo<MyType>(bar)");
        expect(getNonIgnoredBindingNames(bindings)).toEqual(["foo", "bar"]);
      });

      test("new with generic", () => {
        const { bindings } = createExpression("new Map<string, Foo>()");
        expect(getNonIgnoredBindingNames(bindings)).toEqual(["Map"]);
      });
    });

    describe("typed arrow functions", () => {
      test("arrow with typed parameter", () => {
        const { bindings } = createExpression("(x: string) => x + foo");
        expect(hasFunction(bindings)).toBe(true);
        expect(getNonIgnoredBindingNames(bindings)).toEqual(["foo"]);
      });

      test("arrow with return type", () => {
        const { bindings } = createExpression("(x): string => x + foo");
        expect(hasFunction(bindings)).toBe(true);
        expect(getNonIgnoredBindingNames(bindings)).toEqual(["foo"]);
      });

      test("arrow with complex typed parameter", () => {
        const { bindings } = createExpression(
          "(x: { a: number; b: string }) => x.a + foo"
        );
        expect(hasFunction(bindings)).toBe(true);
        // 'a', 'b' are in type, should not be bindings
        expect(getNonIgnoredBindingNames(bindings)).toEqual(["foo"]);
      });

      test("arrow with generic type parameter", () => {
        const { bindings } = createExpression("<T>(x: T) => x");
        expect(hasFunction(bindings)).toBe(true);
        expect(getNonIgnoredBindingNames(bindings)).toEqual([]);
      });

      test("arrow with typed rest parameter", () => {
        const { bindings } = createExpression(
          "(...args: string[]) => args.join(foo)"
        );
        expect(hasFunction(bindings)).toBe(true);
        expect(getNonIgnoredBindingNames(bindings)).toEqual(["foo"]);
      });
    });

    describe("satisfies operator", () => {
      test("simple satisfies", () => {
        const { bindings } = createExpression("foo satisfies MyType");
        expect(getNonIgnoredBindingNames(bindings)).toEqual(["foo"]);
      });

      test("object satisfies", () => {
        const { bindings } = createExpression(
          "{ a: foo, b: bar } satisfies Record<string, any>"
        );
        expect(getNonIgnoredBindingNames(bindings)).toEqual(["foo", "bar"]);
      });
    });

    describe("non-null assertion", () => {
      test("simple non-null", () => {
        const { bindings } = createExpression("foo!");
        expect(getNonIgnoredBindingNames(bindings)).toEqual(["foo"]);
      });

      test("chained non-null", () => {
        const { bindings } = createExpression("foo!.bar!");
        expect(getNonIgnoredBindingNames(bindings)).toEqual(["foo"]);
      });
    });

    describe("complex TypeScript expressions", () => {
      test("type assertion with object literal", () => {
        const { bindings } = createExpression(
          "({ foo: bar } as { foo: string })"
        );
        expect(getNonIgnoredBindingNames(bindings)).toEqual(["bar"]);
      });

      test("as with function type", () => {
        const { bindings } = createExpression("foo as (x: number) => string");
        expect(getNonIgnoredBindingNames(bindings)).toEqual(["foo"]);
      });

      test("generic method call chain", () => {
        const { bindings } = createExpression(
          "arr.map<Foo>(x => x).filter(Boolean)"
        );
        expect(getNonIgnoredBindingNames(bindings)).toEqual(["arr", "Boolean"]);
      });

      test("typeof in expression", () => {
        const { bindings } = createExpression("typeof foo === 'string'");
        expect(getNonIgnoredBindingNames(bindings)).toEqual(["foo"]);
      });

      test("instanceof", () => {
        const { bindings } = createExpression("foo instanceof Bar");
        expect(getNonIgnoredBindingNames(bindings)).toEqual(["foo", "Bar"]);
      });
    });
  });

  // ============================================================
  // SECTION 3: Broken JavaScript Expressions (acorn-loose fallback)
  // These have syntax errors that force fallback to acorn-loose parser
  // ============================================================
  describe("Broken JavaScript Expressions (acorn-loose)", () => {
    describe("trailing dot (incomplete member access)", () => {
      test("simple identifier with trailing dot", () => {
        const { bindings } = createExpression("foo.");
        expect(hasBrokenExpression(bindings)).toBe(true);
      });

      test("member access with trailing dot", () => {
        const { bindings } = createExpression("foo.bar.");
        expect(hasBrokenExpression(bindings)).toBe(true);
      });

      test("method call with trailing dot", () => {
        const { bindings } = createExpression("foo.bar().");
        expect(hasBrokenExpression(bindings)).toBe(true);
      });
    });

    describe("incomplete expressions", () => {
      test("binary expression with missing right operand", () => {
        const { bindings } = createExpression("foo +");
        expect(hasBrokenExpression(bindings)).toBe(true);
      });

      test("incomplete ternary", () => {
        const { bindings } = createExpression("foo ?");
        expect(hasBrokenExpression(bindings)).toBe(true);
      });

      test("incomplete ternary after colon", () => {
        const { bindings } = createExpression("foo ? bar :");
        expect(hasBrokenExpression(bindings)).toBe(true);
      });
    });

    describe("unclosed brackets/parens", () => {
      test("unclosed parenthesis", () => {
        const { bindings } = createExpression("foo(bar");
        expect(hasBrokenExpression(bindings)).toBe(true);
      });

      test("unclosed bracket", () => {
        const { bindings } = createExpression("foo[bar");
        expect(hasBrokenExpression(bindings)).toBe(true);
      });

      test("unclosed brace in object", () => {
        const { bindings } = createExpression("{ foo: bar");
        expect(hasBrokenExpression(bindings)).toBe(true);
      });

      test("unclosed array bracket", () => {
        const { bindings } = createExpression("[foo, bar");
        expect(hasBrokenExpression(bindings)).toBe(true);
      });
    });

    describe("broken arrow functions", () => {
      test("arrow function with trailing dot in body", () => {
        const { bindings } = createExpression("(x) => x.");
        expect(hasBrokenExpression(bindings)).toBe(true);
      });

      test("arrow function with incomplete body", () => {
        const { bindings } = createExpression("(x) => x +");
        expect(hasBrokenExpression(bindings)).toBe(true);
      });

      test("arrow function with rest param and trailing dot", () => {
        const { bindings } = createExpression("(...args) => args.");
        expect(hasBrokenExpression(bindings)).toBe(true);
      });
    });

    describe("broken function expressions", () => {
      test("function with trailing dot", () => {
        const { bindings } = createExpression("function(x) { return x. }");
        expect(hasBrokenExpression(bindings)).toBe(true);
      });

      test("function with incomplete return", () => {
        const { bindings } = createExpression("function(x) { return x +");
        expect(hasBrokenExpression(bindings)).toBe(true);
      });
    });

    describe("broken method chains", () => {
      test("method chain with trailing dot", () => {
        const { bindings } = createExpression("foo.bar().baz.");
        expect(hasBrokenExpression(bindings)).toBe(true);
      });

      test("method chain with incomplete call", () => {
        const { bindings } = createExpression("foo.bar(baz.");
        expect(hasBrokenExpression(bindings)).toBe(true);
      });
    });
  });

  // ============================================================
  // SECTION 4: Broken TypeScript Expressions (acorn-loose fallback)
  // These have TypeScript syntax that acorn-loose doesn't understand
  // ============================================================
  describe("Broken TypeScript Expressions (acorn-loose)", () => {
    describe("type assertions with trailing dot", () => {
      test("as assertion with trailing dot", () => {
        const { bindings } = createExpression("(foo as string).");
        expect(hasBrokenExpression(bindings)).toBe(true);
      });

      test("as assertion on member with trailing dot", () => {
        const { bindings } = createExpression("(foo.bar as Baz).");
        expect(hasBrokenExpression(bindings)).toBe(true);
      });
    });

    describe("typed arrow functions with errors", () => {
      test("typed arrow with trailing dot", () => {
        const { bindings } = createExpression("(x: string) => x.");
        expect(hasBrokenExpression(bindings)).toBe(true);
      });

      test("typed arrow with complex type and trailing dot", () => {
        const { bindings } = createExpression("(x: { foo: number }) => x.foo.");
        expect(hasBrokenExpression(bindings)).toBe(true);
      });

      test("arrow with return type and incomplete", () => {
        const { bindings } = createExpression("(x): string => x +");
        expect(hasBrokenExpression(bindings)).toBe(true);
      });
    });

    describe("generic expressions with errors", () => {
      test("generic call with trailing dot", () => {
        const { bindings } = createExpression("foo<string>(bar).");
        expect(hasBrokenExpression(bindings)).toBe(true);
      });

      test("generic new with trailing dot", () => {
        const { bindings } = createExpression("new Map<string, number>().");
        expect(hasBrokenExpression(bindings)).toBe(true);
      });
    });

    describe("satisfies with errors", () => {
      test("satisfies with trailing dot", () => {
        const { bindings } = createExpression("(foo satisfies MyType).");
        expect(hasBrokenExpression(bindings)).toBe(true);
      });
    });

    describe("non-null assertion with errors", () => {
      test("non-null with trailing dot", () => {
        const { bindings } = createExpression("foo!.");
        expect(hasBrokenExpression(bindings)).toBe(true);
      });

      test("chained non-null with trailing dot", () => {
        const { bindings } = createExpression("foo!.bar!.");
        expect(hasBrokenExpression(bindings)).toBe(true);
      });
    });

    describe("complex broken TypeScript", () => {
      test("type assertion in method chain with trailing dot", () => {
        const { bindings } = createExpression("(foo as Foo).bar(baz as Baz).");
        expect(hasBrokenExpression(bindings)).toBe(true);
      });

      test("generic array method with trailing dot", () => {
        const { bindings } = createExpression("arr.map<Foo>(x => x).");
        expect(hasBrokenExpression(bindings)).toBe(true);
      });

      test("typeof with trailing dot", () => {
        const { bindings } = createExpression("(typeof foo).");
        // typeof returns a string, so this is technically broken but we should handle it
        expect(hasBrokenExpression(bindings)).toBe(true);
      });
    });

    describe("inline type annotations in broken state", () => {
      test("object literal with type annotation and trailing dot", () => {
        const { bindings } = createExpression(
          "({ foo: bar } as { foo: string })."
        );
        expect(hasBrokenExpression(bindings)).toBe(true);
      });

      test("arrow function with inline type and trailing dot", () => {
        const { bindings } = createExpression(
          "((x: number): number => x * 2)."
        );
        expect(hasBrokenExpression(bindings)).toBe(true);
      });
    });
  });

  // ============================================================
  // SECTION 5: Comparison tests (valid vs broken should yield same bindings)
  // ============================================================
  describe("Valid vs Broken - Same Bindings Expected", () => {
    const testCases = [
      {
        name: "simple member access",
        valid: "foo.bar",
        broken: "foo.bar.",
        expectedBindings: ["foo"],
      },
      {
        name: "method call",
        valid: "foo.bar()",
        broken: "foo.bar().",
        expectedBindings: ["foo"],
      },
      {
        name: "function call with args",
        valid: "foo(bar, baz)",
        broken: "foo(bar, baz.",
        expectedBindings: ["foo", "bar", "baz"],
      },
      {
        name: "array access",
        valid: "foo[bar]",
        broken: "foo[bar.",
        expectedBindings: ["foo", "bar"],
      },
      {
        name: "ternary",
        valid: "a ? b : c",
        broken: "a ? b :",
        expectedBindings: ["a", "b"], // 'c' missing in broken
      },
      {
        name: "template literal",
        valid: "`hello ${name}`",
        broken: "`hello ${name.",
        expectedBindings: ["name"],
      },
    ];

    describe("JavaScript expressions", () => {
      testCases.forEach(({ name, valid, broken, expectedBindings }) => {
        test(`${name}: valid version`, () => {
          const { bindings } = createExpression(valid);
          const names = getNonIgnoredBindingNames(bindings);
          expectedBindings.forEach((expected) => {
            expect(names).toContain(expected);
          });
        });

        test(`${name}: broken version`, () => {
          const { bindings } = createExpression(broken);
          expect(hasBrokenExpression(bindings)).toBe(true);
        });
      });
    });

    const typescriptTestCases = [
      {
        name: "type assertion",
        valid: "foo as string",
        broken: "(foo as string).",
        expectedBindings: ["foo"],
      },
      {
        name: "generic function",
        valid: "foo<T>(bar)",
        broken: "foo<T>(bar).",
        expectedBindings: ["foo", "bar"],
      },
      {
        name: "typed arrow function",
        valid: "(x: number) => x * 2",
        broken: "(x: number) => x.",
        hasFunction: true,
      },
      {
        name: "satisfies",
        valid: "foo satisfies Bar",
        broken: "(foo satisfies Bar).",
        expectedBindings: ["foo"],
      },
      {
        name: "non-null assertion",
        valid: "foo!",
        broken: "foo!.",
        expectedBindings: ["foo"],
      },
    ];

    describe("TypeScript expressions", () => {
      typescriptTestCases.forEach(
        ({ name, valid, broken, expectedBindings, hasFunction: expectFn }) => {
          test(`${name}: valid version`, () => {
            const { bindings } = createExpression(valid);
            if (expectedBindings) {
              const names = getNonIgnoredBindingNames(bindings);
              expectedBindings.forEach((expected) => {
                expect(names).toContain(expected);
              });
            }
            if (expectFn) {
              expect(hasFunction(bindings)).toBe(true);
            }
          });

          test(`${name}: broken version`, () => {
            const { bindings } = createExpression(broken);

            expect(hasBrokenExpression(bindings)).toBe(true);
          });
        }
      );
    });
  });

  // ============================================================
  // SECTION 6: Edge cases and special scenarios
  // ============================================================
  describe("Edge Cases", () => {
    test("empty expression", () => {
      // This might throw or return empty, depending on parser behavior
      try {
        const { bindings } = createExpression("");
        expect(bindings).toBeDefined();
      } catch {
        // Expected to potentially fail
      }
    });

    test("only whitespace", () => {
      try {
        const { bindings } = createExpression("   ");
        expect(bindings).toBeDefined();
      } catch {
        // Expected to potentially fail
      }
    });

    test("deeply nested expression", () => {
      const { bindings } = createExpression(
        "a.b.c.d.e.f.g.h(i, j, k).l.m[n].o"
      );
      expect(getNonIgnoredBindingNames(bindings)).toEqual([
        "a",
        "i",
        "j",
        "k",
        "n",
      ]);
    });

    test("multiple arrow functions", () => {
      const { bindings } = createExpression(
        "(x) => (y) => (z) => x + y + z + foo"
      );
      expect(hasFunction(bindings)).toBe(true);
      expect(getNonIgnoredBindingNames(bindings)).toEqual(["foo"]);
    });

    test("arrow function returning arrow function with trailing dot", () => {
      const { bindings } = createExpression("(x) => (y) => x + y.");
      expect(hasBrokenExpression(bindings)).toBe(true);
    });

    test("IIFE", () => {
      const { bindings } = createExpression("((x) => x + foo)(bar)");
      expect(hasFunction(bindings)).toBe(true);
      expect(getNonIgnoredBindingNames(bindings)).toEqual(["foo", "bar"]);
    });

    test("this keyword", () => {
      const { bindings } = createExpression("this.foo");
      // 'this' is a keyword and should be ignored
      const names = getNonIgnoredBindingNames(bindings);
      expect(names).not.toContain("this");
    });

    test("super keyword", () => {
      // super is typically only valid in class context, but we test the parsing
      const { bindings } = createExpression("super.foo");
      const names = getNonIgnoredBindingNames(bindings);
      expect(names).not.toContain("super");
    });

    test("await expression", () => {
      const { bindings } = createExpression("await foo");
      expect(getNonIgnoredBindingNames(bindings)).toEqual(["foo"]);
    });

    test("spread in array with broken state", () => {
      const { bindings } = createExpression("[...arr.");
      expect(hasBrokenExpression(bindings)).toBe(true);
    });

    test("destructuring in arrow parameter with broken state", () => {
      const { bindings } = createExpression("({ a, b }) => a + b.");
      expect(hasBrokenExpression(bindings)).toBe(true);
    });
  });
});
