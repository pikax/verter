import type {
  ParseResult,

  // AST NODE
  Expression,
  PropertyKey,
  MemberExpression,
  Argument,
  AssignmentTarget,
  SimpleAssignmentTarget,
  AssignmentTargetMaybeDefault,
  AssignmentTargetProperty,
  ChainElement,
  Statement,
  Declaration,
  ForStatementInit,
  ForStatementLeft,
  BindingPattern,
  ClassElement,
  ImportDeclarationSpecifier,
  ImportAttributeKey,
  ExportDefaultDeclarationKind,
  ModuleExportName,
  TSLiteral,
  TSType,
  TSTupleElement,
  TSSignature,
  TSTypePredicateName,
  TSModuleDeclarationBody,
  TSTypeQueryExprName,
  TSModuleReference,
  JSXExpression,
  JSXAttributeName,
  JSXAttributeValue,
  JSXChild,
  CharacterClassContents,
  JSXElementName,
  JSXMemberExpressionObject,
  VariableDeclaration,
  VariableDeclarator,
  BindingRestElement,
  BindingProperty,
  TSTypeAliasDeclaration,
  CallExpression,
  ExportSpecifier,
  Function,
  FunctionBody,
  ObjectProperty,
  ArrowFunctionExpression,
  ObjectExpression,
  AwaitExpression,
  ExpressionStatement,
  ExportDefaultDeclaration
} from "oxc-parser";

export type {
  VariableDeclaration,
  VariableDeclarator,
  BindingRestElement,
  BindingPattern,
  Function as FunctionDeclaration,
  Class as ClassDeclaration,
  ExpressionStatement,
  Statement,
  ModuleDeclaration,
  TSTypeAliasDeclaration,
  ImportDeclaration,
  ImportDeclarationSpecifier,
  CallExpression,
  ExportNamedDeclaration,
  Declaration,
  ExportDefaultDeclarationKind,
  ExportAllDeclaration,
  ExportDefaultDeclaration,
  ExportSpecifier,
  FunctionBody,
  ObjectProperty,
  Function,
  ArrowFunctionExpression,
  ObjectExpression,
  AwaitExpression,
} from "oxc-parser";

export type VerterAST = ParseResult["program"];

export type VerterASTNode =
  | Expression
  | PropertyKey
  | MemberExpression
  | Argument
  | AssignmentTarget
  | SimpleAssignmentTarget
  | AssignmentTargetMaybeDefault
  | AssignmentTargetProperty
  | ChainElement
  | Statement
  | Declaration
  | ForStatementInit
  | ForStatementLeft
  | BindingRestElement
  | BindingPattern
  | ClassElement
  | ImportDeclarationSpecifier
  | ImportAttributeKey
  | ExportDefaultDeclarationKind
  | ModuleExportName
  | TSLiteral
  | TSType
  | TSTupleElement
  | TSSignature
  | TSTypePredicateName
  | TSModuleDeclarationBody
  | TSTypeQueryExprName
  | TSModuleReference
  | JSXExpression
  | JSXAttributeName
  | JSXAttributeValue
  | JSXChild
  | CharacterClassContents
  | JSXElementName
  | JSXMemberExpressionObject
  | VariableDeclaration
  | VariableDeclarator
  | BindingProperty
  | TSTypeAliasDeclaration
  | ExportSpecifier
  | FunctionBody
  | ObjectProperty
  | Function
  | ArrowFunctionExpression
  | ObjectExpression
  | AwaitExpression
  | ExpressionStatement
  | ExportDefaultDeclaration
  | CallExpression;
