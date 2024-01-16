import {
  TSTypeAliasDeclaration,
  TSTypeParameterDeclaration,
} from "@babel/types";
import { PluginOption, LocationType } from "../types.js";

import { parse } from "@babel/parser";

export default {
  name: "Generic",
  process(context) {
    if (!context.generic) return;
    const source = context.script?.content;
    if (!source) return;

    const genericCode = `type __GENERIC__<${context.generic}> = {}`;

    try {
      const ast = parse(genericCode, {
        sourceType: "module",
        plugins: ["typescript"],
      });

      const params =
        (ast.program.body[0] as TSTypeAliasDeclaration)?.typeParameters
          ?.params ?? [];

      const items = params.map((param, index) => ({
        name: param.name,
        content: source.slice(param.start!, param.end!),
        index,
      }));

      return {
        type: LocationType.Generic,
        node: undefined,
        items,
      };
    } catch (e) {
      console.error("e");
      return;
    }
  },
} satisfies PluginOption;
