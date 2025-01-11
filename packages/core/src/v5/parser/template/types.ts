import type {
  Node,
  CommentNode,
  AttributeNode,
  DirectiveNode,
  ElementNode,
  ExpressionNode,
  InterpolationNode,
  SimpleExpressionNode,
} from "@vue/compiler-core";
import type * as babel_types from "@babel/types";
import { VerterNode } from "../walk";

export const enum TemplateTypes {
  Binding = "Binding",
  Comment = "Comment",
  Text = "Text",
  Interpolation = "Interpolation",
  Prop = "Prop",
  Element = "Element",

  Directive = "Directive",

  // Template = "Template",
  SlotRender = "SlotRender",
  SlotDeclaration = "SlotDeclaration",
  Condition = "Condition",
  Loop = "Loop",

  Function = "Function",
}

export type TemplateComment = {
  type: TemplateTypes.Comment;
  content: string;
  node: CommentNode;
};

export type TemplateInterpolation = {
  type: TemplateTypes.Interpolation;
  content: string;
  node: InterpolationNode;
};

export type TemplateText = {
  type: TemplateTypes.Text;
  content: string;
  node: Node;
};

export type TemplateFunction = {
  type: TemplateTypes.Function;
  // node: (
  //   | babel_types.FunctionDeclaration
  //   | babel_types.ArrowFunctionExpression
  //   | babel_types.FunctionExpression
  // ) & {
  //   loc: Node["loc"] & babel_types.Node["loc"];
  // };
  node:
    | Omit<babel_types.Node, "loc"> & {
        loc: Node["loc"] & babel_types.Node["loc"];
      };

  body:
    | Omit<babel_types.Node, "loc"> & {
        loc: Node["loc"] & babel_types.Node["loc"];
      };

  context: Record<string, any>;
};

export type TemplateBinding = { type: TemplateTypes.Binding } & (
  | {
      node:
        | Node
        | (Omit<babel_types.Node, "loc"> & {
            loc: Node["loc"] & babel_types.Node["loc"];
          });
      name: string;

      parent: babel_types.Node | null;

      /**
       * if this is a local binding, passed by ignoreBindings
       */
      ignore: boolean;

      directive: null | DirectiveNode;

      exp: SimpleExpressionNode | null;
    }
  | {
      node:
        | Node
        | (Omit<babel_types.Node, "loc"> & {
            loc: Node["loc"] & babel_types.Node["loc"];
          });

      name: undefined;

      parent: null;

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
      
      exp: SimpleExpressionNode | null;

      // directive?: null;
    }
);

export type TemplateProp = { type: TemplateTypes.Prop } & (
  | {
      event: false;
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
      event: false;

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

  element: ElementNode;
};

export type TemplateSlot = {
  type: TemplateTypes.SlotDeclaration;
} & (
  | {
      node: DirectiveNode;
      name: null | TemplateBinding;

      props: null;
      parent: ElementNode;
    }
  | {
      node: ElementNode;

      name: TemplateProp | null;
      props: null | string | TemplateProp[];

      parent: ElementNode;
    }
);

export type TemplateRenderSlot = {
  type: TemplateTypes.SlotRender;
} & {
  prop: TemplateProp | TemplateDirective;
  parent: ElementNode | null;
  element: VerterNode;

  name: null | string | TemplateBinding[];
  context: Record<string, any>;

  condition: TemplateCondition | null;
};

export type TemplateCondition = {
  type: TemplateTypes.Condition;
  node: DirectiveNode;

  bindings: TemplateBinding[];
  element: ElementNode;
  parent: VerterNode;

  context: Record<string, any>;

  siblings: TemplateCondition[];
};

export type TemplateLoop = {
  type: TemplateTypes.Loop;
  node: DirectiveNode & Required<Pick<DirectiveNode, "forParseResult">>;

  element: VerterNode;
  parent: VerterNode;
  context: Record<string, any>;
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
  slot: TemplateSlot | TemplateRenderSlot | null;

  context: ElementContext;
};

export type TemplateItemByType = {
  [TemplateTypes.Binding]: TemplateBinding;
  [TemplateTypes.Comment]: TemplateComment;
  [TemplateTypes.Interpolation]: TemplateInterpolation;
  [TemplateTypes.Text]: TemplateText;
  [TemplateTypes.Prop]: TemplateProp;
  [TemplateTypes.Element]: TemplateElement;
  [TemplateTypes.Directive]: TemplateDirective;
  [TemplateTypes.SlotDeclaration]: TemplateSlot;
  [TemplateTypes.SlotRender]: TemplateRenderSlot;
  [TemplateTypes.Condition]: TemplateCondition;
  [TemplateTypes.Loop]: TemplateLoop;
  [TemplateTypes.Function]: TemplateFunction;
};

export type TemplateItem =
  | TemplateComment
  | TemplateText
  | TemplateBinding
  | TemplateProp
  | TemplateDirective
  | TemplateSlot
  | TemplateRenderSlot
  | TemplateCondition
  | TemplateLoop
  | TemplateInterpolation
  | TemplateElement
  | TemplateFunction;

export type ElementContext = {
  conditions: TemplateCondition[];
  inFor: boolean;
  ignoredIdentifiers: string[];
};
