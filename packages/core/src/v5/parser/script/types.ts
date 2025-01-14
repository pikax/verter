import {
  ArrowFunctionExpression,
  AwaitExpression,
  CallExpression,
  ExportAllDeclaration,
  ExportNamedDeclaration,
  ExpressionStatement,
  FunctionDeclaration,
  ImportDeclaration,
  VerterASTNode,
} from "../ast";

export const enum ScriptTypes {
  /**
   * in Setup is any declaration
   * in Options is data/props/computed/methods/slots or the return from setup
   */
  Binding = "Binding",

  Import = "Import",

  FunctionCall = "FunctionCall",

  Declaration = "Declaration",

  Async = "Async",
  Export = "Export",

  // /**
  //  * Vue macro, eg: defineProps, defineEmits, withDefaults, defineExpose, etc
  //  */
  // Macro = "Macro",

  // // Macros shared between Options and Composition API

  // /**
  //  * defineOptions or component options
  //  */
  // Options = "Options",

  // /**
  //  * defineProps/withDefaults or component.props
  //  */
  // Props = "Props",

  // /**
  //  * defineEmits or component.emits
  //  */
  // Emits = "Emits",

  // /**
  //  * defineExpose or component.expose
  //  */
  // Expose = "Expose",

  // /**
  //  * defineSlots or component.slots
  //  */
  // Slots = "Slots",

  Error = "Error",
  Warning = "Warning",
}

export type ScriptAsync = {
  type: ScriptTypes.Async;
  /**
   * OptionsAPI: async setup() {}
   * CompositionAPI: first await
   */
  node: AwaitExpression | (ArrowFunctionExpression | FunctionDeclaration);

  isAsync: boolean;
};

export type ScriptBinding = {
  type: ScriptTypes.Binding;
  node: VerterASTNode;

  name: string;
};

export type ScriptImport = {
  type: ScriptTypes.Import;
  node: ImportDeclaration;

  /**
   * Resolved bindings, for example, import { a, b as c } from 'module'
   * bindings = {'a', 'c'}
   */
  bindings: ScriptBinding[];
};

export type ScriptExport = {
  type: ScriptTypes.Export;
  node: ExportAllDeclaration | ExportNamedDeclaration;
};

export type ScriptFunctionCall = {
  type: ScriptTypes.FunctionCall;
  node: CallExpression;
  parent: ExpressionStatement;

  name: string;
};

export type ScriptErrorMessage = "EXPORT_DEFAULT_SETUP" | "NO_RETURN_IN_SETUP";
export type ScriptWarningMessage =
  | "NO_MACROS_OUTSIDE_SETUP"
  | "NO_MACROS_INSIDE_FUNCTIONS";

export type ScriptError = {
  type: ScriptTypes.Error;

  message: ScriptErrorMessage;
} & (
  | {
      node: VerterASTNode;
      loc: null;
    }
  | {
      node: null;
      loc: {
        start: number;
        end: number;
      };
    }
);

export type ScriptWarning = {
  type: ScriptTypes.Warning;

  message: ScriptErrorMessage;
} & (
  | {
      node: VerterASTNode;
      loc: null;
    }
  | {
      node: null;
      loc: {
        start: number;
        end: number;
      };
    }
);

export type ScriptDeclaration = {
  type: ScriptTypes.Declaration;
  node: VerterASTNode;
  parent: VerterASTNode;
} & (
  | {
      name: string;
      rest: false;
    }
  | {
      name: null;
      rest: VerterASTNode;
    }
);

export type ScriptItemByType = {
  [ScriptTypes.Async]: ScriptAsync;
  [ScriptTypes.Binding]: ScriptBinding;
  [ScriptTypes.Import]: ScriptImport;
  [ScriptTypes.FunctionCall]: ScriptFunctionCall;
  [ScriptTypes.Export]: ScriptExport;
  [ScriptTypes.Declaration]: ScriptDeclaration;
  [ScriptTypes.Error]: ScriptError;
  [ScriptTypes.Warning]: ScriptWarning;
};

export type ScriptItem =
  | ScriptAsync
  | ScriptBinding
  | ScriptImport
  | ScriptFunctionCall
  | ScriptExport
  | ScriptDeclaration

  // warnings and errors
  | ScriptError
  | ScriptWarning;
