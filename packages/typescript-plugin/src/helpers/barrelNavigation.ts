import type tsModule from "typescript/lib/tsserverlibrary";
import { isVue } from "@verter/language-shared";

export interface AliasedDefinitionInfo {
  fileName: string;
  textSpan: tsModule.TextSpan;
  contextSpan?: tsModule.TextSpan;
  originalTextSpan?: tsModule.TextSpan;
  name: string;
  kind: tsModule.ScriptElementKind;
  containerKind: tsModule.ScriptElementKind;
  containerName: string;
  isLocal: boolean;
  isAmbient: boolean;
  unverified: boolean;
}

export interface AliasedNavigationResult {
  textSpan: tsModule.TextSpan;
  definitions: AliasedDefinitionInfo[];
}

type SourceFileLookup = (fileName: string) => tsModule.SourceFile | undefined;
type DefinitionLike = Pick<AliasedDefinitionInfo, "fileName" | "textSpan" | "contextSpan" | "name">;

function getTokenAtPosition(
  ts: typeof tsModule,
  sourceFile: tsModule.SourceFile,
  position: number,
): tsModule.Node | undefined {
  const runtimeTs = ts as typeof tsModule & {
    getTouchingPropertyName?: (
      sourceFile: tsModule.SourceFile,
      position: number,
    ) => tsModule.Node | undefined;
    getTokenAtPosition?: (
      sourceFile: tsModule.SourceFile,
      position: number,
    ) => tsModule.Node | undefined;
  };

  return (
    runtimeTs.getTouchingPropertyName?.(sourceFile, position) ??
    runtimeTs.getTokenAtPosition?.(sourceFile, position)
  );
}

function getAliasedSymbol(
  ts: typeof tsModule,
  checker: tsModule.TypeChecker,
  sourceFile: tsModule.SourceFile,
  position: number,
): { token: tsModule.Node; symbol: tsModule.Symbol } | undefined {
  const token = getTokenAtPosition(ts, sourceFile, position);
  if (!token) {
    return undefined;
  }

  const symbol = checker.getSymbolAtLocation(token);
  if (!symbol || (symbol.flags & ts.SymbolFlags.Alias) === 0) {
    return undefined;
  }

  const aliased = checker.getAliasedSymbol(symbol);
  if (!aliased || !aliased.declarations?.length) {
    return undefined;
  }

  return { token, symbol: aliased };
}

function toTextSpan(start: number, end: number): tsModule.TextSpan {
  return {
    start,
    length: Math.max(0, end - start),
  };
}

function getNamedSpan(ts: typeof tsModule, declaration: tsModule.Declaration): tsModule.TextSpan {
  const namedDecl = declaration as tsModule.NamedDeclaration;
  const name = namedDecl.name;
  if (name) {
    return toTextSpan(name.getStart(), name.getEnd());
  }

  if (ts.isExportAssignment(declaration)) {
    return toTextSpan(declaration.getStart(), declaration.getEnd());
  }

  return toTextSpan(declaration.getStart(), declaration.getEnd());
}

function getContextSpan(declaration: tsModule.Declaration): tsModule.TextSpan {
  const contextNode = declaration.parent ?? declaration;
  return toTextSpan(contextNode.getStart(), contextNode.getEnd());
}

function getContainerName(ts: typeof tsModule, declaration: tsModule.Declaration): string {
  let current: tsModule.Node | undefined = declaration.parent;
  while (current) {
    const named = current as tsModule.NamedDeclaration;
    if (named.name && ts.isIdentifier(named.name)) {
      return named.name.text;
    }
    current = current.parent;
  }
  return "";
}

function getDeclarationKind(
  ts: typeof tsModule,
  declaration: tsModule.Declaration,
): tsModule.ScriptElementKind {
  if (ts.isClassDeclaration(declaration)) return ts.ScriptElementKind.classElement;
  if (ts.isInterfaceDeclaration(declaration)) return ts.ScriptElementKind.interfaceElement;
  if (ts.isTypeAliasDeclaration(declaration)) return ts.ScriptElementKind.typeElement;
  if (ts.isEnumDeclaration(declaration)) return ts.ScriptElementKind.enumElement;
  if (ts.isFunctionDeclaration(declaration)) return ts.ScriptElementKind.functionElement;
  if (ts.isMethodDeclaration(declaration)) return ts.ScriptElementKind.memberFunctionElement;
  if (ts.isGetAccessorDeclaration(declaration)) {
    return ts.ScriptElementKind.memberGetAccessorElement;
  }
  if (ts.isSetAccessorDeclaration(declaration)) {
    return ts.ScriptElementKind.memberSetAccessorElement;
  }
  if (ts.isPropertyDeclaration(declaration) || ts.isPropertySignature(declaration)) {
    return ts.ScriptElementKind.memberVariableElement;
  }
  if (ts.isVariableDeclaration(declaration)) return ts.ScriptElementKind.constElement;
  if (ts.isParameter(declaration)) return ts.ScriptElementKind.parameterElement;
  if (ts.isModuleDeclaration(declaration)) return ts.ScriptElementKind.moduleElement;
  return ts.ScriptElementKind.unknown;
}

function isAmbientDeclaration(ts: typeof tsModule, declaration: tsModule.Declaration): boolean {
  return (
    (ts.getCombinedModifierFlags(declaration as tsModule.DeclarationStatement) &
      ts.ModifierFlags.Ambient) !==
    0
  );
}

function buildDefinitionInfo(
  ts: typeof tsModule,
  symbol: tsModule.Symbol,
  declaration: tsModule.Declaration,
): AliasedDefinitionInfo {
  return {
    fileName: declaration.getSourceFile().fileName,
    textSpan: getNamedSpan(ts, declaration),
    contextSpan: getContextSpan(declaration),
    name: symbol.getName(),
    kind: getDeclarationKind(ts, declaration),
    containerKind: ts.ScriptElementKind.unknown,
    containerName: getContainerName(ts, declaration),
    isLocal: true,
    isAmbient: isAmbientDeclaration(ts, declaration),
    unverified: false,
  };
}

export function getAliasedNavigationResult(
  ts: typeof tsModule,
  checker: tsModule.TypeChecker,
  sourceFile: tsModule.SourceFile,
  position: number,
): AliasedNavigationResult | undefined {
  const resolved = getAliasedSymbol(ts, checker, sourceFile, position);
  if (!resolved) {
    return undefined;
  }

  const declarations = resolved.symbol.declarations ?? [];
  const seen = new Set<string>();
  const definitions = declarations
    .map((declaration) => buildDefinitionInfo(ts, resolved.symbol, declaration))
    .filter((definition) => {
      const key = `${definition.fileName}:${definition.textSpan.start}:${definition.textSpan.length}`;
      if (seen.has(key)) {
        return false;
      }
      seen.add(key);
      return true;
    });

  if (definitions.length === 0) {
    return undefined;
  }

  return {
    textSpan: toTextSpan(resolved.token.getStart(sourceFile), resolved.token.getEnd()),
    definitions,
  };
}

export function getAliasedQuickInfo(
  ts: typeof tsModule,
  languageService: Pick<tsModule.LanguageService, "getQuickInfoAtPosition">,
  checker: tsModule.TypeChecker,
  sourceFile: tsModule.SourceFile,
  position: number,
): tsModule.QuickInfo | undefined {
  const result = getAliasedNavigationResult(ts, checker, sourceFile, position);
  if (!result?.definitions.length) {
    return undefined;
  }

  const target = result.definitions[0];
  const quickInfo = languageService.getQuickInfoAtPosition(target.fileName, target.textSpan.start);
  if (!quickInfo) {
    return undefined;
  }

  return {
    ...quickInfo,
    textSpan: result.textSpan,
  };
}

function collectNamedIdentifierPositions(
  ts: typeof tsModule,
  sourceFile: tsModule.SourceFile,
  span: tsModule.TextSpan,
  name: string,
): number[] {
  const positions: number[] = [];
  const spanEnd = span.start + span.length;

  const visit = (node: tsModule.Node): void => {
    const start = node.getStart(sourceFile);
    const end = node.getEnd();
    if (end < span.start || start > spanEnd) {
      return;
    }

    if (ts.isIdentifier(node) && node.text === name) {
      positions.push(start);
    }

    ts.forEachChild(node, visit);
  };

  visit(sourceFile);
  return positions;
}

function getAliasedDefinitionsForDefinition(
  ts: typeof tsModule,
  checker: tsModule.TypeChecker,
  sourceFile: tsModule.SourceFile,
  definition: Pick<AliasedDefinitionInfo, "textSpan" | "contextSpan" | "name">,
  preferredName?: string,
): AliasedDefinitionInfo[] | undefined {
  const direct = getAliasedNavigationResult(
    ts,
    checker,
    sourceFile,
    definition.textSpan.start,
  )?.definitions;
  if (direct?.length) {
    return direct;
  }

  const targetName =
    definition.name && !definition.name.startsWith('"') ? definition.name : preferredName;
  if (!targetName) {
    return undefined;
  }

  const searchSpan = definition.contextSpan ?? definition.textSpan;
  for (const position of collectNamedIdentifierPositions(ts, sourceFile, searchSpan, targetName)) {
    const aliased = getAliasedNavigationResult(ts, checker, sourceFile, position)?.definitions;
    if (aliased?.length) {
      return aliased;
    }
  }

  return undefined;
}

export function retargetAliasedDefinitionInfos<T extends DefinitionLike>(
  ts: typeof tsModule,
  checker: tsModule.TypeChecker,
  getSourceFile: SourceFileLookup,
  definitions: readonly T[] | undefined,
  preferredName?: string,
): T[] | undefined {
  if (!definitions?.length) {
    return undefined;
  }

  const retargeted: T[] = [];
  const seen = new Set<string>();

  for (const definition of definitions) {
    const sourceFile = getSourceFile(definition.fileName);
    const aliased = sourceFile
      ? getAliasedDefinitionsForDefinition(ts, checker, sourceFile, definition, preferredName)
      : undefined;
    const candidates = aliased?.length ? aliased : [definition];
    for (const candidate of candidates) {
      const key = `${candidate.fileName}:${candidate.textSpan.start}:${candidate.textSpan.length}`;
      if (seen.has(key)) {
        continue;
      }
      seen.add(key);
      retargeted.push(candidate as T);
    }
  }

  return retargeted;
}

export function getModuleSpecifierNavigationResult(
  ts: typeof tsModule,
  sourceFile: tsModule.SourceFile,
  position: number,
  resolveModule: (moduleName: string) => string | undefined,
): AliasedNavigationResult | undefined {
  const token = getTokenAtPosition(ts, sourceFile, position);
  if (!token) {
    return undefined;
  }

  const literal = ts.isStringLiteral(token)
    ? token
    : token.parent && ts.isStringLiteral(token.parent)
      ? token.parent
      : undefined;
  if (!literal) {
    return undefined;
  }

  const parent = literal.parent;
  if (
    !(
      (ts.isImportDeclaration(parent) || ts.isExportDeclaration(parent)) &&
      parent.moduleSpecifier === literal
    )
  ) {
    return undefined;
  }

  if (!isVue(literal.text)) {
    return undefined;
  }

  const resolvedFileName = resolveModule(literal.text);
  if (!resolvedFileName) {
    return undefined;
  }

  return {
    textSpan: toTextSpan(literal.getStart(sourceFile) + 1, literal.getEnd() - 1),
    definitions: [
      {
        fileName: resolvedFileName,
        textSpan: toTextSpan(0, 1),
        contextSpan: toTextSpan(0, 1),
        name: literal.text,
        kind: ts.ScriptElementKind.moduleElement,
        containerKind: ts.ScriptElementKind.unknown,
        containerName: "",
        isLocal: false,
        isAmbient: false,
        unverified: false,
      },
    ],
  };
}
