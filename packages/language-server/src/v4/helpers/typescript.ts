import ts from "typescript";
import {
  CompletionItem,
  CompletionItemKind,
  CompletionTriggerKind,
  type Diagnostic,
  DiagnosticSeverity,
  DiagnosticTag,
  InsertTextFormat,
  Range,
} from "vscode-languageserver/node";
import { VueSubDocument } from "../documents";

export function mapCompletion(
  tsCompletion: ts.CompletionEntry,
  data: {
    virtualUrl: string;
    index: number;
    triggerKind: CompletionTriggerKind | undefined;
    triggerCharacter: string | undefined;
  }
) {
  const item: CompletionItem = {
    label: tsCompletion.name,
    kind:
      retrieveKindBasedOnSymbol(tsCompletion.symbol) ||
      KindMap[tsCompletion.kind] ||
      CompletionItemKind.Text,
    detail: tsCompletion.source, // Example mapping, adjust as needed
    insertText: tsCompletion.insertText || tsCompletion.name,
    insertTextFormat: InsertTextFormat.PlainText,
    sortText: tsCompletion.sortText,
    filterText: tsCompletion.filterText,
    documentation: tsCompletion.source,
    data: Object.assign(data, tsCompletion.data),
  };

  // todo add extra or process completion

  return item;
}

export function itemToMarkdown(item: ts.CompletionEntryDetails | ts.QuickInfo) {
  return {
    kind: "markdown" as "markdown",
    value:
      ts.displayPartsToString(item.documentation) +
      (item.tags
        ? "\n\n" +
          item.tags
            .map(
              (tag) =>
                `_@${tag.name}_ — \`${ts.displayPartsToString(
                  tag.text?.slice(0, 1)
                )}\` ${ts.displayPartsToString(tag.text?.slice(1))}\n`
            )
            .join("\n")
        : ""),
  };
}

export function mapDiagnostic(
  diagnostic: ts.Diagnostic,
  document: VueSubDocument
): Diagnostic | undefined {
  const range = mapTextSpanToRange(diagnostic, document);
  if (!range) {
    return;
  }

  const severity = categoryToSeverity(diagnostic.category);

  return {
    severity,
    range,
    source: "Verter",
    code: diagnostic.code,

    message: ts.flattenDiagnosticMessageText(diagnostic.messageText, "\n"),

    tags: [
      diagnostic.reportsUnnecessary && DiagnosticTag.Unnecessary,
      diagnostic.reportsDeprecated && DiagnosticTag.Deprecated,
    ].filter(Boolean) as DiagnosticTag[],
  };
}

export function formatQuickInfo(
  quickInfo: ts.QuickInfo,
  document: VueSubDocument
) {
  const contents = itemToMarkdown(quickInfo);

  console.log("sssd", quickInfo.displayParts);

  const str =
    "```ts\n" +
    displayPartsToString(quickInfo.displayParts) +
    "\n```\n" +
    "\n\n" +
    contents.value;

  // remove the verter virtual
  // contents.value = str.replace(/verter-virtual:\/\/\//gi, "");
  contents.value = str;

  // Convert the text span to an LSP range
  const range = mapTextSpanToRange(quickInfo.textSpan, document);

  return {
    contents,
    range,
  };
}

function displayPartsToString(parts: ts.SymbolDisplayPart[]) {
  return ts.displayPartsToString(
    parts.map((x) => {
      // patch the verter-virtual paths
      if (x.kind === "stringLiteral") {
        let text = x.text;
        if (
          text.startsWith('"verter-virtual:///') ||
          text.startsWith("'verter-virtual:///")
        ) {
          return {
            ...x,
            text:
              x.text.slice(0, 1) +
              decodeURIComponent(x.text).slice("'verter-virtual:///".length),
          };
        } else if (text.startsWith("verter-virtual:///")) {
          console.log("found textX ", text);
          debugger;
        }
      }
      return x;
    })
  );
}

function mapTextSpanToRange(
  span: ts.TextSpan,
  document: VueSubDocument
): Range | undefined {
  const start = document.toOriginalPositionFromOffset(span.start);
  const end = document.toOriginalPositionFromOffset(span.start + span.length);
  if (start.line - 1 || end.line - 1) {
    return undefined;
  }
  return Range.create(start, end);
}

function categoryToSeverity(
  category: ts.DiagnosticCategory
): DiagnosticSeverity {
  switch (category) {
    case ts.DiagnosticCategory.Error: {
      return DiagnosticSeverity.Error;
    }
    case ts.DiagnosticCategory.Warning: {
      return DiagnosticSeverity.Warning;
    }

    case ts.DiagnosticCategory.Message: {
      return DiagnosticSeverity.Information;
    }
    case ts.DiagnosticCategory.Suggestion: {
      return DiagnosticSeverity.Hint;
    }
  }
}

const KindMap: Record<ts.ScriptElementKind, CompletionItemKind> = {
  [ts.ScriptElementKind.unknown]: CompletionItemKind.Text,
  [ts.ScriptElementKind.warning]: CompletionItemKind.Text,
  [ts.ScriptElementKind.keyword]: CompletionItemKind.Keyword,
  [ts.ScriptElementKind.scriptElement]: CompletionItemKind.File,
  [ts.ScriptElementKind.moduleElement]: CompletionItemKind.Module,
  [ts.ScriptElementKind.classElement]: CompletionItemKind.Class,
  [ts.ScriptElementKind.localClassElement]: CompletionItemKind.Class,
  [ts.ScriptElementKind.interfaceElement]: CompletionItemKind.Interface,
  [ts.ScriptElementKind.typeElement]: CompletionItemKind.Class, // No direct equivalent in LSP; Class is a reasonable approximation.
  [ts.ScriptElementKind.enumElement]: CompletionItemKind.Enum,
  [ts.ScriptElementKind.enumMemberElement]: CompletionItemKind.EnumMember,
  [ts.ScriptElementKind.variableElement]: CompletionItemKind.Variable,
  [ts.ScriptElementKind.localVariableElement]: CompletionItemKind.Variable,
  [ts.ScriptElementKind.functionElement]: CompletionItemKind.Function,
  [ts.ScriptElementKind.localFunctionElement]: CompletionItemKind.Function,
  [ts.ScriptElementKind.memberFunctionElement]: CompletionItemKind.Method,
  [ts.ScriptElementKind.memberGetAccessorElement]: CompletionItemKind.Property,
  [ts.ScriptElementKind.memberSetAccessorElement]: CompletionItemKind.Property,
  [ts.ScriptElementKind.memberVariableElement]: CompletionItemKind.Field,
  [ts.ScriptElementKind.constructorImplementationElement]:
    CompletionItemKind.Constructor,
  [ts.ScriptElementKind.callSignatureElement]: CompletionItemKind.Method,
  [ts.ScriptElementKind.indexSignatureElement]: CompletionItemKind.Property,
  [ts.ScriptElementKind.constructSignatureElement]:
    CompletionItemKind.Constructor,
  [ts.ScriptElementKind.parameterElement]: CompletionItemKind.Variable,
  [ts.ScriptElementKind.typeParameterElement]: CompletionItemKind.TypeParameter,
  [ts.ScriptElementKind.primitiveType]: CompletionItemKind.Class,
  [ts.ScriptElementKind.label]: CompletionItemKind.Text,
  [ts.ScriptElementKind.alias]: CompletionItemKind.Reference,
  [ts.ScriptElementKind.constElement]: CompletionItemKind.Constant,
  [ts.ScriptElementKind.letElement]: CompletionItemKind.Variable,
  [ts.ScriptElementKind.directory]: CompletionItemKind.Folder,
  [ts.ScriptElementKind.externalModuleName]: CompletionItemKind.Module,
  [ts.ScriptElementKind.jsxAttribute]: CompletionItemKind.Property,
  [ts.ScriptElementKind.string]: CompletionItemKind.Constant,
  [ts.ScriptElementKind.link]: CompletionItemKind.Reference,
  [ts.ScriptElementKind.linkName]: CompletionItemKind.Reference,
  [ts.ScriptElementKind.linkText]: CompletionItemKind.Text,

  [ts.ScriptElementKind.variableAwaitUsingElement]: CompletionItemKind.Variable,
  [ts.ScriptElementKind.variableUsingElement]: CompletionItemKind.Variable,
  [ts.ScriptElementKind.memberAccessorVariableElement]:
    CompletionItemKind.Property,
};

function retrieveKindBasedOnSymbol(
  symbol?: ts.Symbol
): CompletionItemKind | null {
  if (!symbol) return null;
  const flags = symbol.getFlags();

  if (flags & ts.SymbolFlags.Function) {
    return CompletionItemKind.Function;
  } else if (flags & ts.SymbolFlags.Method) {
    return CompletionItemKind.Method;
  } else if ((flags & ts.SymbolFlags.Class) !== 0) {
    return CompletionItemKind.Class;
  } else if ((flags & ts.SymbolFlags.Interface) !== 0) {
    return CompletionItemKind.Interface;
  } else if (
    (flags & ts.SymbolFlags.Enum) !== 0 ||
    (flags & ts.SymbolFlags.ConstEnum) !== 0 ||
    (flags & ts.SymbolFlags.RegularEnum) !== 0
  ) {
    return CompletionItemKind.Enum;
  } else if (
    (flags & ts.SymbolFlags.ValueModule) !== 0 ||
    (flags & ts.SymbolFlags.NamespaceModule) !== 0
  ) {
    return CompletionItemKind.Module;
  } else if (
    (flags & ts.SymbolFlags.Variable) !== 0 ||
    (flags & ts.SymbolFlags.BlockScopedVariable) !== 0 ||
    (flags & ts.SymbolFlags.FunctionScopedVariable) !== 0
  ) {
    return CompletionItemKind.Variable;
  } else if (flags & ts.SymbolFlags.ClassMember) {
    return CompletionItemKind.Class;
  } else if (
    (flags & ts.SymbolFlags.Property) !== 0 ||
    (flags & ts.SymbolFlags.Accessor) !== 0
  ) {
    return CompletionItemKind.Property;
  } else if ((flags & ts.SymbolFlags.GetAccessor) !== 0) {
    return CompletionItemKind.Property; // No direct mapping for getters; Property is a reasonable approximation
  } else if ((flags & ts.SymbolFlags.SetAccessor) !== 0) {
    return CompletionItemKind.Property; // Similar to GetAccessor
  } else if ((flags & ts.SymbolFlags.EnumMember) !== 0) {
    return CompletionItemKind.EnumMember;
  } else if ((flags & ts.SymbolFlags.TypeAlias) !== 0) {
    return CompletionItemKind.TypeParameter; // Using TypeParameter for TypeAlias as a close match
  } else if ((flags & ts.SymbolFlags.Alias) !== 0) {
    return CompletionItemKind.Reference; // Using Reference for Alias
  }

  return null;
}
