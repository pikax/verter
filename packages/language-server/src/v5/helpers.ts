import ts from "typescript";
import { VueSubDocument } from "./documents/verter/vue/sub/sub";
import {
  CompletionItem,
  CompletionItemKind,
  CompletionTriggerKind,
  Definition,
  DefinitionLink,
  type Diagnostic,
  DiagnosticSeverity,
  DiagnosticTag,
  InsertTextFormat,
  Location,
  Position,
  Range,
} from "vscode-languageserver/node";
import { DocumentManager, isVueDocument, pathToUri } from "./documents";

export function formatQuickInfo(
  quickInfo: ts.QuickInfo,
  document: VueSubDocument
) {
  const contents = itemToMarkdown(quickInfo);

  const str =
    "```ts\n" +
    ts.displayPartsToString(quickInfo.displayParts) +
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
function mapTextSpanToRange(
  span: ts.TextSpan,
  document: VueSubDocument
): Range | undefined {
  const start = document.toOriginalPositionFromOffset(span.start);
  const end = document.toOriginalPositionFromOffset(span.start + span.length);
  if (start.line === -1 || end.line === -1) {
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

export function mapDefinitionInfo(
  info: ts.DefinitionInfo,
  documentManager: DocumentManager,
  fallback = false
): (DefinitionLink & { key: string }) | undefined {
  const filename = info.fileName;

  const doc = documentManager.getDocument(filename);
  if (!doc) return;

  let contextRange = info.contextSpan
    ? Range.create(
        doc.positionAt(info.contextSpan.start),
        doc.positionAt(info.contextSpan.start + info.contextSpan.length)
      )
    : undefined;

  let textSpan : Range | undefined = Range.create(
    doc.positionAt(info.textSpan.start),
    doc.positionAt(info.textSpan.start + info.textSpan.length)
  );

  if (isVueDocument(doc)) {
    const subDoc = doc.getSubDoc(filename);
    if (!subDoc) return;

    if (info.contextSpan) {
      contextRange = mapTextSpanToRange(info.contextSpan, subDoc);
    }

    textSpan = mapTextSpanToRange(info.textSpan, subDoc);
  }
  if (!textSpan) {
    if (!fallback) return undefined;
    textSpan = Range.create(Position.create(0, 0), Position.create(0, 0));
  }
  const targetUri = pathToUri(doc.uri);

  return {
    key: `${targetUri}:${textSpan.start.line}:${textSpan.start.character}`,

    targetUri,
    targetRange: contextRange ?? textSpan,
    targetSelectionRange: textSpan,
  };
}
