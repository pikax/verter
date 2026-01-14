import {
  parseOXC,
  parseAcornLoose,
  sanitisePosition,
  parseAST,
} from "./ast.js";
import { bench } from "vitest";
import { MagicString, walk } from "@vue/compiler-sfc";
import { basename } from "node:path";
import { isFunctionType } from "@vue/compiler-core";

describe("parse AST", () => {
  describe("OXC", () => {
    function parse(source: string) {
      return parseOXC(source);
    }

    it("should parse", () => {
      const source = `const a = 0;`;
      const ast = parse(source);

      expect(ast.program.body).toMatchObject([
        {
          kind: "const",
          start: 0,
          end: 12,
          declarations: [
            {
              type: "VariableDeclarator",
              start: 6,
              end: 11,
            },
          ],
        },
      ]);
    });

    it("should contain errors", () => {
      const source = `const a = 'a`;
      const ast = parse(source);
      expect(ast.errors).toHaveLength(1);
      expect(ast.errors[0].message).toBe("Unterminated string");
      expect(ast.errors[0].severity).toBe("Error");
      expect(ast.errors[0].labels).toHaveLength(1);
      expect(ast.errors[0].labels[0]).toEqual({
        end: 12,
        start: 10,
      });
    });

    describe("non-ascii", () => {
      it("oxc has issues with the correct position on non-ascii", () => {
        const sourceA = `const a = '金';`;
        const sourceB = `const b = 'a';`;

        let source = [sourceA, sourceB].join("\n");
        const parsed = parse(source);

        const [a, b] = parsed.program.body;
        expect(source.slice(a.start, a.end)).toBe(sourceA);
        expect(source.slice(b.start, b.end)).toBe(sourceB);
      });

      it("sanitisePosition should work correctly", () => {
        const sourceA = `const a = '金';`;
        const sourceB = `const b = 'a';`;

        const source = [sourceA, sourceB].join("\n");
        const parsed = parse(sanitisePosition(source));

        const [a, b] = parsed.program.body;
        expect(source.slice(a.start, a.end)).toBe(sourceA);
        expect(source.slice(b.start, b.end)).toBe(sourceB);
      });
    });
  });

  describe("AcornLoose", () => {
    function parse(source: string) {
      return parseAcornLoose(source);
    }

    it("should parse", () => {
      const source = `const a = 0;`;
      const ast = parse(source);

      expect(ast.body).toMatchObject([
        {
          kind: "const",
          start: 0,
          end: 12,
          declarations: [
            {
              type: "VariableDeclarator",
              start: 6,
              end: 11,
            },
          ],
        },
      ]);
    });

    it("should handle incomplete", () => {
      const source = `const a = 'a`;
      const ast = parse(source);

      expect(ast.body).toMatchObject([
        {
          kind: "const",
          start: 0,
          end: 12,
          declarations: [
            {
              type: "VariableDeclarator",
              start: 6,
              end: 12,
            },
          ],
        },
      ]);
    });

    describe("non-ascii", () => {
      // note if this starts working sanitisation is not necessary
      it("correct position on non-ascii", () => {
        const sourceA = `const a = '金';`;
        const sourceB = `const b = 'a';`;

        const source = [sourceA, sourceB].join("\n");
        const parsed = parse(source);

        const [a, b] = parsed.body;
        expect(source.slice(a.start, a.end)).toBe(sourceA);
        expect(source.slice(b.start, b.end)).toBe(sourceB);
      });

      it("sanitisePosition should work correctly", () => {
        const sourceA = `const a = '金';`;
        const sourceB = `const b = 'a';`;

        const source = [sourceA, sourceB].join("\n");
        const parsed = parse(source);

        const [a, b] = parsed.body;
        expect(source.slice(a.start, a.end)).toBe(sourceA);
        expect(source.slice(b.start, b.end)).toBe(sourceB);
      });
    });
  });

  describe("parseAST", () => {
    it("should parse", () => {
      const source = `const a = 0;`;
      const ast = parseAST(source);

      expect(ast.body).toMatchObject([
        {
          kind: "const",
          start: 0,
          end: 12,
          declarations: [
            {
              type: "VariableDeclarator",
              start: 6,
              end: 11,
            },
          ],
        },
      ]);
    });

    it("should handle incomplete", () => {
      const source = `const a = 'a`;
      const ast = parseAST(source);

      expect(ast.body).toMatchObject([
        {
          kind: "const",
          start: 0,
          end: 12,
          declarations: [
            {
              type: "VariableDeclarator",
              start: 6,
              end: 12,
            },
          ],
        },
      ]);
    });

    describe("non-ascii", () => {
      // note if this starts working sanitisation is not necessary
      it("correct position on non-ascii", () => {
        const sourceA = `const a = '金';`;
        const sourceB = `const b = 'a';`;

        const source = [sourceA, sourceB].join("\n");
        const parsed = parseAST(source);

        const [a, b] = parsed.body;
        expect(source.slice(a.start, a.end)).toBe(sourceA);
        expect(source.slice(b.start, b.end)).toBe(sourceB);
      });

      it("sanitisePosition should work correctly", () => {
        const sourceA = `const a = '金';`;
        const sourceB = `const b = 'a';`;

        const source = [sourceA, sourceB].join("\n");
        const parsed = parseAST(sanitisePosition(source));

        const [a, b] = parsed.body;
        expect(source.slice(a.start, a.end)).toBe(sourceA);
        expect(source.slice(b.start, b.end)).toBe(sourceB);
      });
    });
  });
});
