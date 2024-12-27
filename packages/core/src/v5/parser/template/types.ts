import { Node } from "@vue/compiler-core";
import * as babel_types from "@babel/types";

export type TemplateBinding = {
  node:
    | Node
    | (Omit<babel_types.Node, "loc"> & {
        loc: Node["loc"] & babel_types.Node["loc"];
      });
  name: string;

  ignore: boolean;
};
