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
                type: "Property",
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
                type: "Property",
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
                type: "Property",
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
                type: "Property",
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
                type: "Property",
                start: 15,
                end: 16,
              },
            },
          ],
        });
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
                type: "Property",
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
                type: "Property",
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
                type: "Property",
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
                type: "Property",
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
                type: "Property",
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
                type: "Property",
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
                end: 30,
              },
            },
          ],
        });
      });
    });
  });

  describe("imports", () => {
    it.todo("should parse imports", () => {
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
              end: 26,
            },
            node: {
              type: "ImportSpecifier",
              start: 7,
              end: 8,
            },
          },
          {
            content: "b",
            name: "b",
            parent: {
              type: "ImportDeclaration",
              start: 0,
              end: 26,
            },
            node: {
              type: "ImportSpecifier",
              start: 10,
              end: 11,
            },
          },
        ],
      });
    });
  });
});
