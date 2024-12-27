import declaration from "../../../plugins/declaration/declaration.js";
import { parseAST, parseOXC } from "../ast/ast.js";
import { parseScript } from "./index.js";

describe("parser script", () => {
  function parse(source: string) {
    const ast = parseOXC(source);
    return parseScript(ast.program, source);
  }

  describe("isAsync", () => {
    it("not async", () => {
      const result = parse(`foo;`);

      expect(result.isAsync).toBe(false);
    });

    it("async", () => {
      const result = parse(`await foo;`);

      expect(result.isAsync).toBe(true);
    });

    it("async on a function", () => {
      const result = parse(`const a = async ()=> { await Promise.resolve()}`);
      expect(result.isAsync).toBe(false);
    });
  });

  describe("declarations", () => {
    test("var", () => {
      const source = `var x = 10`;
      const result = parse(source);

      expect(result).toMatchObject({
        declarations: [
          {
            content: "x = 10",
            name: "x",
            parent: {
              type: "VariableDeclaration",
              kind: "var",
              start: 0,
              end: 10,
            },
            node: {
              type: "VariableDeclarator",
              start: 4,
              end: 10,
            },
          },
        ],
      });
    });
    test("let", () => {
      const source = `let x = 10`;
      const result = parse(source);

      expect(result).toMatchObject({
        declarations: [
          {
            content: "x = 10",
            name: "x",
            parent: {
              type: "VariableDeclaration",
              kind: "let",
              start: 0,
              end: 10,
            },
            node: {
              type: "VariableDeclarator",
              start: 4,
              end: 10,
            },
          },
        ],
      });
    });
    test("const", () => {
      const source = `const x = 10`;
      const result = parse(source);
      expect(result).toMatchObject({
        declarations: [
          {
            content: "x = 10",
            name: "x",
            parent: {
              type: "VariableDeclaration",
              kind: "const",
              start: 0,
              end: 12,
            },
            node: {
              type: "VariableDeclarator",
              start: 6,
              end: 12,
            },
          },
        ],
      });
    });
    test("function", () => {
      const source = `function myFunction() {}`;
      const result = parse(source);

      expect(result).toMatchObject({
        declarations: [
          {
            content: source,
            name: "myFunction",
            node: {
              type: "FunctionDeclaration",
              start: 0,
              end: 24,
            },
          },
        ],
      });
    });
    test("function expression", () => {
      const source = `const myFunction = function () { }`;
      const result = parse(source);

      expect(result).toMatchObject({
        declarations: [
          {
            content: "myFunction = function () { }",
            name: "myFunction",
            parent: {
              type: "VariableDeclaration",
              kind: "const",
              start: 0,
              end: 34,
            },
            node: {
              type: "VariableDeclarator",
              start: 6,
              end: 34,
            },
          },
        ],
      });
    });
    test("arrow function", () => {
      const source = `const myFunction = () => { }`;
      const result = parse(source);

      expect(result).toMatchObject({
        declarations: [
          {
            content: "myFunction = () => { }",
            name: "myFunction",
            parent: {
              type: "VariableDeclaration",
              kind: "const",
              start: 0,
              end: 28,
            },
            node: {
              type: "VariableDeclarator",
              start: 6,
              end: 28,
            },
          },
        ],
      });
    });
    test("generator function", () => {
      const source = `function* myFunction() { }`;
      const result = parse(source);

      expect(result).toMatchObject({
        declarations: [
          {
            content: source,
            name: "myFunction",
            node: {
              type: "FunctionDeclaration",
              start: 0,
              end: 26,
            },
          },
        ],
      });
    });
    test("async function", () => {
      const source = `async function myFunction() { const a = 1}`;
      const result = parse(source);

      expect(result).toMatchObject({
        declarations: [
          {
            content: source,
            name: "myFunction",
            node: {
              type: "FunctionDeclaration",
              start: 0,
              end: 42,
            },
          },
        ],
      });
    });

    test("arrow arrow function", () => {
      const source = `const myFunction = async () => { }`;
      const result = parse(source);

      expect(result).toMatchObject({
        declarations: [
          {
            content: "myFunction = async () => { }",
            name: "myFunction",
            parent: {
              type: "VariableDeclaration",
              kind: "const",
              start: 0,
              end: 34,
            },
            node: {
              type: "VariableDeclarator",
              start: 6,
              end: 34,
            },
          },
        ],
      });
    });

    test("class", () => {
      const source = `class MyClass {}`;
      const result = parse(source);

      expect(result).toMatchObject({
        declarations: [
          {
            content: source,
            name: "MyClass",
            node: {
              type: "ClassDeclaration",
              start: 0,
              end: 16,
            },
          },
        ],
      });
    });
    test("class expression", () => {
      const source = `const MyClass = class {}`;
      const result = parse(source);

      expect(result).toMatchObject({
        declarations: [
          {
            content: "MyClass = class {}",
            name: "MyClass",
            parent: {
              type: "VariableDeclaration",
              kind: "const",
              start: 0,
              end: 24,
            },
            node: {
              type: "VariableDeclarator",
              start: 6,
              end: 24,
            },
          },
        ],
      });
    });

    test("class without Identifier id", () => {
      const source = `class {}`;
      const result = parse(source);

      expect(result.declarations).toMatchObject([
        {
          content: "class {}",
          name: "",
          node: {
            type: "ClassDeclaration",
            start: 0,
            end: 8,
          },
        },
      ]);
    });

    describe("deconstructor", () => {
      test("object", () => {
        const source = `const { a, b } = foo`;
        const result = parse(source);

        expect(result).toMatchObject({
          declarations: [
            {
              content: "a",
              name: "a",
              parent: {
                type: "VariableDeclaration",
                kind: "const",
                start: 0,
                end: 20,
              },
              node: {
                type: "BindingProperty",
                start: 8,
                end: 9,
              },
            },
            {
              content: "b",
              name: "b",
              parent: {
                type: "VariableDeclaration",
                kind: "const",
                start: 0,
                end: 20,
              },
              node: {
                type: "BindingProperty",
                start: 11,
                end: 12,
              },
            },
          ],
        });
      });
      test("object rest", () => {
        const source = `const { a, ...others } = foo`;
        const result = parse(source);

        expect(result).toMatchObject({
          declarations: [
            {
              content: "a",
              name: "a",
              parent: {
                type: "VariableDeclaration",
                kind: "const",
                start: 0,
                end: 28,
              },
              node: {
                type: "BindingProperty",
                start: 8,
                end: 9,
              },
            },
            {
              content: "...others",
              name: "others",
              parent: {
                type: "VariableDeclaration",
                kind: "const",
                start: 0,
                end: 28,
              },
              node: {
                type: "RestElement",
                start: 11,
                end: 20,
              },
            },
          ],
        });
      });
      test("object assignment", () => {
        const source = `const { a = 1, b } = foo`;
        const result = parse(source);

        expect(result).toMatchObject({
          declarations: [
            {
              content: "a = 1",
              name: "a",
              parent: {
                type: "VariableDeclaration",
                kind: "const",
                start: 0,
                end: 24,
              },
              node: {
                type: "BindingProperty",
                start: 8,
                end: 13,
              },
            },
            {
              content: "b",
              name: "b",
              parent: {
                type: "VariableDeclaration",
                kind: "const",
                start: 0,
                end: 24,
              },
              node: {
                type: "BindingProperty",
                start: 15,
                end: 16,
              },
            },
          ],
        });
      });

      test("object assignment deep", () => {
        const source = `const { a : { c: { d = 1} } , b } = foo`;
        const result = parse(source);

        expect(result).toMatchObject({
          declarations: [
            {
              content: "d = 1",
              name: "d",
              parent: {
                type: "VariableDeclaration",
                kind: "const",
                start: 0,
                end: 39,
              },
              node: {
                type: "BindingProperty",
                start: 19,
                end: 24,
              },
            },
            {
              content: "b",
              name: "b",
              parent: {
                type: "VariableDeclaration",
                kind: "const",
                start: 0,
                end: 39,
              },
              node: {
                type: "BindingProperty",
                start: 30,
                end: 31,
              },
            },
          ],
        });
      });

      test("object pattern with identical key and value", () => {
        const source = `const { a } = foo;`;
        const result = parse(source);

        expect(result.declarations).toMatchObject([
          {
            content: "a",
            name: "a",
            parent: {
              type: "VariableDeclaration",
              kind: "const",
              start: 0,
              end: 18,
            },
            node: {
              type: "BindingProperty",
              start: 8,
              end: 9,
            },
          },
        ]);
      });

      test("object deep", () => {
        const source = `const { a : { test: { bar } }, b } = foo`;
        const result = parse(source);

        expect(result).toMatchObject({
          declarations: [
            {
              content: "bar",
              name: "bar",
              parent: {
                type: "VariableDeclaration",
                kind: "const",
                start: 0,
                end: 40,
              },
              node: {
                type: "BindingProperty",
                start: 22,
                end: 25,
              },
            },
            {
              content: "b",
              name: "b",
              parent: {
                type: "VariableDeclaration",
                kind: "const",
                start: 0,
                end: 40,
              },
              node: {
                type: "BindingProperty",
                start: 31,
                end: 32,
              },
            },
          ],
        });
      });
      test("object assignment deep", () => {
        const source = `const { a : { test = 1 }, b } = foo`;
        const result = parse(source);

        expect(result).toMatchObject({
          declarations: [
            {
              content: "test = 1",
              name: "test",
              parent: {
                type: "VariableDeclaration",
                kind: "const",
                start: 0,
                end: 35,
              },
              node: {
                type: "BindingProperty",
                start: 14,
                end: 22,
              },
            },
            {
              content: "b",
              name: "b",
              parent: {
                type: "VariableDeclaration",
                kind: "const",
                start: 0,
                end: 35,
              },
              node: {
                type: "BindingProperty",
                start: 26,
                end: 27,
              },
            },
          ],
        });
      });

      test("array", () => {
        const source = `const [ a, b ] = foo`;
        const result = parse(source);

        expect(result).toMatchObject({
          declarations: [
            {
              content: "a",
              name: "a",
              parent: {
                type: "VariableDeclaration",
                kind: "const",
                start: 0,
                end: 20,
              },
              node: {
                type: "Identifier",
                start: 8,
                end: 9,
              },
            },
            {
              content: "b",
              name: "b",
              parent: {
                type: "VariableDeclaration",
                kind: "const",
                start: 0,
                end: 20,
              },
              node: {
                type: "Identifier",
                start: 11,
                end: 12,
              },
            },
          ],
        });
      });

      test("array assignment", () => {
        const source = `const [ a = 1, b ] = foo`;
        const result = parse(source);

        expect(result).toMatchObject({
          declarations: [
            {
              content: "a = 1",
              name: "a",
              parent: {
                type: "VariableDeclaration",
                kind: "const",
                start: 0,
                end: 24,
              },
              node: {
                type: "AssignmentPattern",
                start: 8,
                end: 13,
              },
            },
            {
              content: "b",
              name: "b",
              parent: {
                type: "VariableDeclaration",
                kind: "const",
                start: 0,
                end: 24,
              },
              node: {
                type: "Identifier",
                start: 15,
                end: 16,
              },
            },
          ],
        });
      });

      test("assignment pattern with no right items", () => {
        const source = `const [a = undefined] = foo;`;
        const result = parse(source);

        expect(result.declarations).toMatchObject([
          {
            content: "a = undefined",
            name: "a",
            parent: {
              type: "VariableDeclaration",
              kind: "const",
              start: 0,
              end: 28,
            },
            node: {
              type: "AssignmentPattern",
              start: 7,
              end: 20,
            },
          },
        ]);
      });

      test("array rest", () => {
        const source = `const [ a, ...others ] = foo`;
        const result = parse(source);

        expect(result).toMatchObject({
          declarations: [
            {
              content: "a",
              name: "a",
              parent: {
                type: "VariableDeclaration",
                kind: "const",
                start: 0,
                end: 28,
              },
              node: {
                type: "Identifier",
                start: 8,
                end: 9,
              },
            },
            {
              content: "...others",
              name: "others",
              parent: {
                type: "VariableDeclaration",
                kind: "const",
                start: 0,
                end: 28,
              },
              node: {
                type: "RestElement",
                start: 11,
                end: 20,
              },
            },
          ],
        });
      });

      test("array assignment", () => {
        const source = `const [ a = 1, b ] = foo`;
        const result = parse(source);

        expect(result).toMatchObject({
          declarations: [
            {
              content: "a = 1",
              name: "a",
              parent: {
                type: "VariableDeclaration",
                kind: "const",
                start: 0,
                end: 24,
              },
              node: {
                type: "AssignmentPattern",
                start: 8,
                end: 13,
              },
            },
            {
              content: "b",
              name: "b",
              parent: {
                type: "VariableDeclaration",
                kind: "const",
                start: 0,
                end: 24,
              },
              node: {
                type: "Identifier",
                start: 15,
                end: 16,
              },
            },
          ],
        });
      });

      test("array deep", () => {
        const source = `const [{ test: { bar } }, b ] = foo`;
        const result = parse(source);

        expect(result).toMatchObject({
          declarations: [
            {
              content: "bar",
              name: "bar",
              parent: {
                type: "VariableDeclaration",
                kind: "const",
                start: 0,
                end: 35,
              },
              node: {
                type: "BindingProperty",
                start: 17,
                end: 20,
              },
            },
            {
              content: "b",
              name: "b",
              parent: {
                type: "VariableDeclaration",
                kind: "const",
                start: 0,
                end: 35,
              },
              node: {
                type: "Identifier",
                start: 26,
                end: 27,
              },
            },
          ],
        });
      });
      test("array assignment deep", () => {
        const source = `const [{ test = 1 }, b ] = foo`;
        const result = parse(source);

        expect(result).toMatchObject({
          declarations: [
            {
              content: "test = 1",
              name: "test",
              parent: {
                type: "VariableDeclaration",
                kind: "const",
                start: 0,
                end: 30,
              },
              node: {
                type: "BindingProperty",
                start: 9,
                end: 17,
              },
            },
            {
              content: "b",
              name: "b",
              parent: {
                type: "VariableDeclaration",
                kind: "const",
                start: 0,
                end: 30,
              },
              node: {
                type: "Identifier",
                start: 21,
                end: 22,
              },
            },
          ],
        });
      });
    });

    describe("typescript", () => {
      test("type", () => {
        const source = `type MyType = { a: string }`;
        const result = parse(source);

        expect(result).toMatchObject({
          declarations: [
            {
              content: source,
              name: "MyType",
              node: {
                type: "TSTypeAliasDeclaration",
                start: 0,
                end: 27,
              },
            },
          ],
        });
      });
      test("declare var", () => {
        const source = `declare var x: number = 10`;
        const result = parse(source);

        expect(result).toMatchObject({
          declarations: [
            {
              content: "x: number = 10",
              name: "x",
              parent: {
                type: "VariableDeclaration",
                kind: "var",
                start: 0,
                end: 26,
              },
              node: {
                type: "VariableDeclarator",
                start: 12,
                end: 26,
              },
            },
          ],
        });
      });

      test("var as number", () => {
        const source = `var x = 10 as number`;
        const result = parse(source);

        expect(result).toMatchObject({
          declarations: [
            {
              content: "x = 10 as number",
              name: "x",
              parent: {
                type: "VariableDeclaration",
                kind: "var",
                start: 0,
                end: 20,
              },
              node: {
                type: "VariableDeclarator",
                start: 4,
                end: 20,
              },
            },
          ],
        });
      });
    });
  });

  describe("imports", () => {
    it("should parse imports", () => {
      const source = `import { a, b } from 'foo';`;
      const result = parse(source);

      expect(result).toMatchObject({
        imports: [
          {
            content: "a",
            name: "a",
            parent: {
              type: "ImportDeclaration",
              start: 0,
              end: 27,
            },
            node: {
              type: "ImportSpecifier",
              start: 9,
              end: 10,
            },
          },
          {
            content: "b",
            name: "b",
            parent: {
              type: "ImportDeclaration",
              start: 0,
              end: 27,
            },
            node: {
              type: "ImportSpecifier",
              start: 12,
              end: 13,
            },
          },
        ],
      });
    });

    it("default", () => {
      const source = `import Foo from 'foo';`;
      const result = parse(source);

      expect(result).toMatchObject({
        imports: [
          {
            content: "Foo",
            name: "Foo",
            parent: {
              type: "ImportDeclaration",
              start: 0,
              end: 22,
            },
            node: {
              type: "ImportDefaultSpecifier",
              start: 7,
              end: 10,
            },
          },
        ],
      });
    });

    it("* as Name", () => {
      const source = `import * as Foo from 'foo';`;
      const result = parse(source);

      expect(result).toMatchObject({
        imports: [
          {
            content: "* as Foo",
            name: "Foo",
            parent: {
              type: "ImportDeclaration",
              start: 0,
              end: 27,
            },
            node: {
              type: "ImportNamespaceSpecifier",
              start: 7,
              end: 15,
            },
          },
        ],
      });
    });

    it("imports with name", () => {
      const source = `import { a as c, b } from 'foo';`;
      const result = parse(source);

      expect(result).toMatchObject({
        imports: [
          {
            content: "a as c",
            name: "c",
            parent: {
              type: "ImportDeclaration",
              start: 0,
              end: 32,
            },
            node: {
              type: "ImportSpecifier",
              start: 9,
              end: 15,
            },
          },
          {
            content: "b",
            name: "b",
            parent: {
              type: "ImportDeclaration",
              start: 0,
              end: 32,
            },
            node: {
              type: "ImportSpecifier",
              start: 17,
              end: 18,
            },
          },
        ],
      });
    });

    it("type imports", () => {
      const source = `import type { a, b } from 'foo';`;
      const result = parse(source);

      expect(result).toMatchObject({
        imports: [
          {
            content: "a",
            name: "a",
            parent: {
              type: "ImportDeclaration",
              start: 0,
              end: 32,
              importKind: "type",
            },
            node: {
              type: "ImportSpecifier",
              start: 14,
              end: 15,
            },
          },
          {
            content: "b",
            name: "b",
            parent: {
              type: "ImportDeclaration",
              start: 0,
              end: 32,
              importKind: "type",
            },
            node: {
              type: "ImportSpecifier",
              start: 17,
              end: 18,
            },
          },
        ],
      });
    });

    it("type default", () => {
      const source = `import type Foo from 'foo';`;
      const result = parse(source);

      expect(result).toMatchObject({
        imports: [
          {
            content: "Foo",
            name: "Foo",
            parent: {
              type: "ImportDeclaration",
              start: 0,
              end: 27,
              importKind: "type",
            },
            node: {
              type: "ImportDefaultSpecifier",
              start: 12,
              end: 15,
            },
          },
        ],
      });
    });

    it("mixed type", () => {
      const source = `import { type a, b } from 'foo';`;
      const result = parse(source);

      expect(result).toMatchObject({
        imports: [
          {
            content: "type a",
            name: "a",
            parent: {
              type: "ImportDeclaration",
              start: 0,
              end: 32,
            },
            node: {
              type: "ImportSpecifier",
              start: 9,
              end: 15,
              importKind: "type",
            },
          },
          {
            content: "b",
            name: "b",
            parent: {
              type: "ImportDeclaration",
              start: 0,
              end: 32,
            },
            node: {
              type: "ImportSpecifier",
              start: 17,
              end: 18,
            },
          },
        ],
      });
    });

    test("mixed import types", () => {
      const source = `import Foo, { type Bar, Baz } from "foo";`;
      const result = parse(source);

      expect(result.imports).toMatchObject([
        {
          content: "Foo",
          name: "Foo",
          node: {
            type: "ImportDefaultSpecifier",
            start: 7,
            end: 10,
          },
        },
        {
          content: "type Bar",
          name: "Bar",
          node: {
            type: "ImportSpecifier",
            start: 14,
            end: 22,
            importKind: "type",
          },
        },
        {
          content: "Baz",
          name: "Baz",
          node: {
            type: "ImportSpecifier",
            start: 24,
            end: 27,
          },
        },
      ]);
    });
  });

  describe("calls", () => {
    it("simple call", () => {
      const source = `defineProps()`;
      const result = parse(source);

      expect(result).toMatchObject({
        calls: [
          {
            content: "defineProps()",
            name: "defineProps",
            node: {
              type: "CallExpression",
              start: 0,
              end: 13,
            },
          },
        ],
      });
    });
  });

  describe("exports", () => {
    it("simple", () => {
      const source = `export const a = 1;`;
      const result = parse(source);

      expect(result).toMatchObject({
        exports: [
          {
            content: "a = 1",
            name: "a",
            default: false,
            parent: {
              type: "ExportNamedDeclaration",
              start: 0,
              end: 19,
            },
            node: {
              type: "VariableDeclarator",
              start: 13,
              end: 18,
            },
          },
        ],
      });
    });

    it("type", () => {
      const source = `export type { a, b } from 'foo';`;
      const result = parse(source);
      expect(result).toMatchObject({
        exports: [
          {
            content: "a",
            name: "a",
            default: false,
            parent: {
              type: "ExportNamedDeclaration",
              start: 0,
              end: 32,
            },
            node: {
              type: "ExportSpecifier",
              start: 14,
              end: 15,
            },
          },
          {
            content: "b",
            name: "b",
            default: false,
            parent: {
              type: "ExportNamedDeclaration",
              start: 0,
              end: 32,
            },
            node: {
              type: "ExportSpecifier",
              start: 17,
              end: 18,
            },
          },
        ],
      });
    });

    it("export * from", () => {
      const source = `export * from 'foo'`;
      const result = parse(source);

      expect(result).toMatchObject({
        exports: [
          {
            content: "export * from 'foo'",
            name: "*",
            default: false,
            node: {
              type: "ExportAllDeclaration",
              start: 0,
              end: 19,
            },
          },
        ],
      });
    });

    describe("default", () => {
      it("function", () => {
        const source = `export default function foo() {}`;
        const result = parse(source);

        expect(result).toMatchObject({
          exports: [
            {
              content: "function foo() {}",
              name: "default",
              default: true,
              parent: {
                type: "ExportDefaultDeclaration",
                start: 0,
                end: 32,
              },
              node: {
                type: "FunctionDeclaration",
                start: 15,
                end: 32,
              },
            },
          ],
        });
      });

      it("expression", () => {
        const source = `export default foo`;
        const result = parse(source);

        expect(result).toMatchObject({
          exports: [
            {
              content: "foo",
              name: "default",
              default: true,
              parent: {
                type: "ExportDefaultDeclaration",
                start: 0,
                end: 18,
              },
              node: {
                type: "Identifier",
                start: 15,
                end: 18,
              },
            },
          ],
        });
      });

      it("expression (object)", () => {
        const source = `export default { foo }`;
        const result = parse(source);

        expect(result).toMatchObject({
          exports: [
            {
              content: "{ foo }",
              name: "default",
              default: true,
              parent: {
                type: "ExportDefaultDeclaration",
                start: 0,
                end: 22,
              },
              node: {
                type: "ObjectExpression",
                start: 15,
                end: 22,
              },
            },
          ],
        });
      });

      it("expression (array)", () => {
        const source = `export default [ foo ]`;
        const result = parse(source);

        expect(result).toMatchObject({
          exports: [
            {
              content: "[ foo ]",
              name: "default",
              default: true,
              parent: {
                type: "ExportDefaultDeclaration",
                start: 0,
                end: 22,
              },
              node: {
                type: "ArrayExpression",
                start: 15,
                end: 22,
              },
            },
          ],
        });
      });

      it("expression (arrow function)", () => {
        const source = `export default () => {}`;
        const result = parse(source);

        expect(result).toMatchObject({
          exports: [
            {
              content: "() => {}",
              name: "default",
              default: true,
              parent: {
                type: "ExportDefaultDeclaration",
                start: 0,
                end: 23,
              },
              node: {
                type: "ArrowFunctionExpression",
                start: 15,
                end: 23,
              },
            },
          ],
        });
      });

      it("expression (class)", () => {
        const source = `export default class Foo {}`;
        const result = parse(source);

        expect(result).toMatchObject({
          exports: [
            {
              content: "class Foo {}",
              name: "default",
              default: true,
              parent: {
                type: "ExportDefaultDeclaration",
                start: 0,
                end: 27,
              },
              node: {
                type: "ClassDeclaration",
                start: 15,
                end: 27,
              },
            },
          ],
        });
      });

      it("expression (expression)", () => {
        const source = `export default foo()`;
        const result = parse(source);

        expect(result).toMatchObject({
          exports: [
            {
              content: "foo()",
              name: "default",
              default: true,
              parent: {
                type: "ExportDefaultDeclaration",
                start: 0,
                end: 20,
              },
              node: {
                type: "CallExpression",
                start: 15,
                end: 20,
              },
            },
          ],
        });
      });
    });
  });
});
