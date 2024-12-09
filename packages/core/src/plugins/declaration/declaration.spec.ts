import { LocationType } from "../types.js";
import DeclarationPlugin from "./index.js";
import babel_types from "@babel/types";

describe("declaration plugin", () => {
  describe("walk", () => {
    it.each([
      ["VariableDeclaration", true],
      ["FunctionDeclaration", true],
      ["EnumDeclaration", true],
      ["ClassDeclaration", true],
      ["InterfaceDeclaration", true],
      ["CommentLine", false],
    ] as Array<[babel_types.Node["type"], boolean]>)(
      "should return %s node",
      (type, expected) => {
        const node = {
          type,
          start: 0,
          end: 1,
        } as babel_types.Statement;

        const r = DeclarationPlugin.walk(node, {
          script: {
            loc: {
              source: "@",
            },
          },
        } as any);

        if (expected) {
          expect(r).toEqual({
            type: LocationType.Declaration,
            generated: false,
            node,
            declaration: {
              content: "@",
            },
          });
        } else {
          expect(r).toBeUndefined();
        }
      }
    );

    it("undefined if not source", () => {
      const node = {
        type: "VariableDeclaration",
        start: 0,
        end: 1,
      } as babel_types.Statement;

      const r = DeclarationPlugin.walk(node, {
        script: {
          loc: {
            source: undefined,
          },
        },
      } as any);

      expect(r).toBeUndefined();
    });

    it('support for typescript type', ()=> {
      
    })
  });
});
