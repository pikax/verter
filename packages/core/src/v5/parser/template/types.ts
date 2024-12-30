import type {
  Node,
  CommentNode,
  AttributeNode,
  DirectiveNode,
  ElementNode,
  ExpressionNode,
} from "@vue/compiler-core";
import type * as babel_types from "@babel/types";
import { VerterNode } from "../walk";

export const enum TemplateTypes {
  Binding = "Binding",
  Comment = "Comment",
  Text = "Text",
  Prop = "Prop",
  Element = "Element",

  Directive = "Directive",

  Template = "Template",
  SlotRender = "SlotRender",
  SlotDeclaration = "SlotDeclaration",
  Condition = "Condition",
  Loop = "Loop",
}

export type TemplateComment = {
  type: TemplateTypes.Comment;
  content: string;
  node: CommentNode;
};

export type TemplateText = {
  type: TemplateTypes.Text;
  content: string;
  node: Node;
};

export type TemplateBinding = { type: TemplateTypes.Binding } & (
  | {
      node:
        | Node
        | (Omit<babel_types.Node, "loc"> & {
            loc: Node["loc"] & babel_types.Node["loc"];
          });
      name: string;

      /**
       * if this is a local binding, passed by ignoreBindings
       */
      ignore: boolean;
    }
  | {
      node:
        | Node
        | (Omit<babel_types.Node, "loc"> & {
            loc: Node["loc"] & babel_types.Node["loc"];
          });

      name: undefined;

      /**
       * Expression value
       */
      value: string;
      /**
       * ignored identifiers for this node
       */
      context: {
        ignoredIdentifiers: string[];
      } & Record<string, any>;
      /**
       * if the name is invalid
       */
      invalid: true;

      ignore: false;
    }
);

export type TemplateProp = { type: TemplateTypes.Prop } & (
  | {
      node: AttributeNode;
      name: string;

      value: string | null;
      static: true;
    }
  | {
      node: DirectiveNode;
      event: boolean;

      name: null | TemplateBinding[];
      value: null | TemplateBinding[];

      static: false;
      context: Record<string, any>;
    }
  | {
      // used to merge styles and classes
      node: null;
      name: string;

      props: TemplateProp[];
    }
);

export type TemplateDirective = {
  type: TemplateTypes.Directive;
  node: DirectiveNode;

  name: string;

  arg: null | TemplateBinding[];
  exp: null | TemplateBinding[];
  context: Record<string, any>;
  static: false;
};

export type TemplateSlot = {
  type: TemplateTypes.SlotDeclaration;
} & (
  | {
      node: DirectiveNode;
      name: null | string | TemplateBinding[];

      props: null;
      parent: ElementNode;
    }
  | {
      node: ElementNode;

      name: TemplateProp | null;
      props: null | string | TemplateProp[];

      parent: null;
    }
);

export type TemplateRenderSlot = {
  type: TemplateTypes.SlotRender;
} & (
  | {
      prop: TemplateProp | TemplateDirective;
      parent: VerterNode;

      name: null | string | TemplateBinding[];
    }
  | {
      prop: null;
    }
);

export type TemplateCondition = {
  type: TemplateTypes.Condition;
  node: DirectiveNode;

  bindings: TemplateBinding[];
  element: ElementNode;
  parent: VerterNode;

  context: Record<string, any>;
};

export type TemplateLoop = {
  type: TemplateTypes.Loop;
  node: DirectiveNode & Required<Pick<DirectiveNode, "forParseResult">>;

  element: VerterNode;
  parent: VerterNode;
};

export type TemplateElement = {
  type: TemplateTypes.Element;

  tag: string;

  node: ElementNode;
  parent: VerterNode;

  ref: TemplateProp | null;
  props: TemplateProp[] | null;

  condition: TemplateCondition | null;
  loop: TemplateLoop | null;
  slot: TemplateSlot | null;
};
