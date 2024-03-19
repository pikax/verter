// import lsp from "vscode-languageserver/lib/node/main.js";
import lsp, { CompletionItem, CompletionItemKind, InsertTextFormat, TextDocuments } from "vscode-languageserver/node";
import { TextDocument } from "vscode-languageserver-textdocument";
import Logger from "./logger";

import { readFile } from "fs/promises";

import { NotificationType, patchClient } from "@verter/language-shared";
import { RequestType } from "@verter/language-shared";

import { createBuilder } from "@verter/core";
import { documentManager } from "./lib/documents/Manager";
import { getTypescriptService } from "./plugins/typescript/TypescriptPlugin";
import { offsetAt } from "./lib/documents/utils";
import { VueDocument } from "./lib/documents/VueDocument";
import ts from "typescript";

// import {
//   ClientCapabilities,
//   DocumentContext,
//   getDefaultHTMLDataProvider,
//   getLanguageService,
//   HTMLDocument,
//   IAttributeData,
//   IHTMLDataProvider,
//   LanguageService,
//   newHTMLDataProvider,
//   Scanner,
//   TokenType,
// } from 'vscode-html-languageservice'



export interface LsConnectionOption {
  /**
   * The connection to use. If not provided, a new connection will be created.
   */
  connection?: lsp.Connection;

  logErrorsOnly?: boolean;
}

export function startServer(options: LsConnectionOption = {}) {
  Logger.error("in startServer", options);
  //   const connection = lsp.createConnection(lsp.ProposedFeatures.all);

  let connection = patchClient(options.connection);
  if (!connection) {
    if (process.argv.includes("--stdio")) {
      console.log = console.warn;
      connection = patchClient(
        lsp.createConnection(process.stdin, process.stdout)
      );
    } else {
      connection = patchClient(lsp.createConnection(lsp.ProposedFeatures.all));
      // new lsp.IPCMessageReader(process),
      // new lsp.IPCMessageWriter(process)
      // );
    }
  }

  if (options?.logErrorsOnly !== undefined) {
    Logger.setLogErrorsOnly(options.logErrorsOnly);
  }

  connection.onInitialized(() => {
    console.log("inited XXX--- lOL");
  });

  connection.onInitialize((params) => {
    // console.log("inited --- lOL");
    // debugger;

    return {
      capabilities: {
        textDocumentSync: {
          openClose: true,
          change: lsp.TextDocumentSyncKind.Incremental,
          save: {
            includeText: false,
          },
        },
        // Tell the client that the server supports code completion
        completionProvider: {
          // allCommitCharacters: true,
          resolveProvider: true,
          completionItem: {
            labelDetailsSupport: true
          },
          triggerCharacters: [".", "@", "<"],
        }, referencesProvider: true,
        // typeDefinitionProvider: true,
        hoverProvider: true,
        diagnosticProvider: true
      },
    };
  });

  connection.onNotification(NotificationType.OnDidChangeTsOrJsFile, (e) => {
    // TODO add/update the file

    console.log("got notification ", NotificationType.OnDidChangeTsOrJsFile, e);
  });
  const builder = createBuilder();

  function sendDiagnostics(document: VueDocument): void {
    const fileName = document.uri;
    if (fileName.startsWith('file:')) return;
    const content = document.template.content;
    // const fileContent = document.getText();

    // TODO cache sourceFile
    // Parse the file content to get a SourceFile object
    // const sourceFile = ts.createSourceFile(fileName, content, ts.ScriptTarget.Latest, true, ts.ScriptKind.TSX);

    const syntacticDiagnostics = tsService.getSyntacticDiagnostics(fileName);
    const semanticDiagnostics = tsService.getSemanticDiagnostics(fileName);
    const suggestionDiagnostics = tsService.getSuggestionDiagnostics(fileName);


    const allDiagnostics = [...syntacticDiagnostics, ...semanticDiagnostics, ...suggestionDiagnostics];
    const lspDiagnostics = allDiagnostics.map(diagnostic => convertTsDiagnosticToLspDiagnostic(diagnostic, document));


    // Get the semantic diagnostics for the source file
    // const diagnostics = ts.getPreEmitDiagnostics(tsService.getProgram(), sourceFile);

    // Convert TypeScript diagnostics to LSP diagnostics
    // const lspDiagnostics: lsp.Diagnostic[] = diagnostics.map(diagnostic => convertTsDiagnosticToLspDiagnostic(diagnostic));

    // Send the diagnostics to the client

    const originalFile = fileName.replace('verter-virtual:', 'file:').replace('.vue.tsx', '.vue')
    connection!.sendDiagnostics({ uri: originalFile, diagnostics: lspDiagnostics });
  }

  function convertTsDiagnosticToLspDiagnostic(diagnostic: ts.Diagnostic, document: VueDocument): lsp.Diagnostic {
    const templateInfo = document.template;



    const start = diagnostic.start ? diagnostic.file?.getLineAndCharacterOfPosition(diagnostic.start) : { line: 0, character: 0 };
    const end = diagnostic.start ? diagnostic.file?.getLineAndCharacterOfPosition(diagnostic.start + diagnostic.length) : { line: 0, character: 1 };

    const mappedStart = templateInfo.mapConsumer.originalPositionFor({
      column: start!.character,
      line: start!.line + 1
    })
    const mappedEnd = templateInfo.mapConsumer.originalPositionFor({
      column: end!.character,
      line: end!.line + 1
    })

    return {
      severity: diagnostic.category === ts.DiagnosticCategory.Error ? lsp.DiagnosticSeverity.Error : lsp.DiagnosticSeverity.Warning,
      range: {
        start: { line: mappedStart.line - 1, character: mappedStart.column },
        end: { line: mappedEnd.line - 1, character: mappedEnd.column },
      },
      message: ts.flattenDiagnosticMessageText(diagnostic.messageText, '\n'),
      source: 'Verter',
    };
  }

  documentManager.addListener((type, params) => {
    const doc = params.document

    if (type === 'close') {
      connection!.sendDiagnostics({ uri: params.document.uri, diagnostics: [] });
      return
    }

    if (doc) {
      sendDiagnostics(doc)
    }
  })

  // connection.onDidChangeTextDocument(change => {
  //   debugger
  //   if (!change.textDocument.uri.endsWith('.vue')) return
  //   const virtualUrl = change.textDocument.uri.replace('file:', 'verter-virtual:').replace('.vue', '.vue.tsx')
  //   const doc = documentManager.getDocument(virtualUrl)
  //   if (doc && 'template' in doc) {
  //     sendDiagnostics(doc)
  //   }
  // })
  // connection.onDidOpenTextDocument(params => {
  //   debugger
  //   if (!params.textDocument.uri.endsWith('.vue')) return
  //   const virtualUrl = params.textDocument.uri.replace('file:', 'verter-virtual:').replace('.vue', '.vue.tsx')
  //   const doc = documentManager.getDocument(virtualUrl)
  //   if (doc && 'template' in doc) {
  //     sendDiagnostics(doc)
  //   }
  // })
  // connection.onDidCloseTextDocument((params) => {
  //   debugger
  //   connection!.sendDiagnostics({ uri: params.textDocument.uri, diagnostics: [] });
  // });

  connection.onRequest(RequestType.GetCompiledCode, async (uri) => {
    console.log(
      "got request ",
      RequestType.GetCompiledCode,
      uri,
      decodeURIComponent(uri)
    );

    if (uri.endsWith('.vue')) {
      uri = uri.replace('file:', 'verter-virtual:').replace('.vue', '.vue.tsx')
    }

    const doc = documentManager.getDocument(uri);
    if (!doc || !('template' in doc)) {
      return {
        js: {
          code: doc?.getText() ?? ''
        }
      }
    }


    // const uriPath = decodeURIComponent(uri).replace("file:///", "");
    console.log("opening ", uri);

    // const content = await readFile(uriPath, "utf-8");

    // const result = await builder.process(uriPath, content);

    // console.log("resovled request ", uriPath, result.length);

    //todo handle doc
    return {
      js: {
        code: doc.template.content,
        map: doc.template.map,
      },
      css: {
        code: "",
        map: "",
      },
    };
  });

  const tsService = getTypescriptService(process.cwd());



  // connection.client.connection


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
    [ts.ScriptElementKind.constructorImplementationElement]: CompletionItemKind.Constructor,
    [ts.ScriptElementKind.callSignatureElement]: CompletionItemKind.Method,
    [ts.ScriptElementKind.indexSignatureElement]: CompletionItemKind.Property,
    [ts.ScriptElementKind.constructSignatureElement]: CompletionItemKind.Constructor,
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
    [ts.ScriptElementKind.memberAccessorVariableElement]: CompletionItemKind.Property
  };

  const r = {} as any as ts.ScriptElementKind
  const a = CompletionItemKind[r]


  function mapTsCompletionToLspItem(tsCompletion: ts.CompletionEntry, data: { virtualUrl: string, index: number, triggerKind: lsp.CompletionTriggerKind | undefined, triggerCharacter: string | undefined }): CompletionItem | CompletionItem[] {
    const lspItem: CompletionItem = {
      label: tsCompletion.name,
      kind: KindMap[tsCompletion.kind] || CompletionItemKind.Text,
      detail: tsCompletion.source, // Example mapping, adjust as needed
      insertText: tsCompletion.insertText || tsCompletion.name,
      insertTextFormat: InsertTextFormat.PlainText,
      sortText: tsCompletion.sortText,
      filterText: tsCompletion.filterText,
      documentation: tsCompletion.source,
      data: Object.assign(data, tsCompletion.data)
    };

    // TODO we should set the insert text to snippet for the template

    const label = lspItem.label;

    if (label.startsWith('on')) {
      lspItem.kind = CompletionItemKind.Method
      // convert event from onInput to @input
      const atEvent: CompletionItem = {
        ...lspItem,
        label: `@${label[2].toLocaleLowerCase()}${label.slice(3)}`,
        insertText: `${label[2].toLocaleLowerCase()}${label.slice(3)}`,
        kind: CompletionItemKind.Method
      }

      if (data.triggerCharacter === '@') {
        return atEvent
      }
      atEvent.insertText = '@' + atEvent.insertText
      return [
        lspItem,
        atEvent
      ]
    }

    // if (tsCompletion.replacementSpan) {
    //   const start = tsCompletion.replacementSpan.start;
    //   const length = tsCompletion.replacementSpan.length;
    //   const startPos = document.positionAt(start);
    //   const endPos = document.positionAt(start + length);

    //   textEdit = TextEdit.replace(Range.create(startPos, endPos), tsCompletion.insertText || tsCompletion.name);
    // }

    return lspItem;
  }


  connection.onCompletionResolve((item) => {
    const data: { virtualUrl: string, index: number, triggerKind: lsp.CompletionTriggerKind | undefined, triggerCharacter: string | undefined } = item.data ?? {}

    const label = item.label.startsWith('@') ? `on${item.label[1].toLocaleUpperCase()}${item.label.slice(2)}` : item.label;

    const details = tsService.getCompletionEntryDetails(data.virtualUrl, data.index, label, undefined, undefined, undefined, item.data)
    if (!details) return item

    item.detail = ts.displayPartsToString(details.displayParts)

    // item.documentation = {
    //   kind: 'markdown',
    //   value: ts.displayPartsToString(details.documentation) + (details.tags ? '\n\n' + details.tags.map(tag => `_@${tag.name}_ — \`${ts.displayPartsToString(tag.text?.slice(0, 1))}\` ${ts.displayPartsToString(tag.text?.slice(1))}\n`).join('\n') : '')
    // }
    item.documentation = generateMarkdown(details)

    return item;
  })

  function generateMarkdown(item: ts.CompletionEntryDetails | ts.QuickInfo): lsp.MarkupContent {
    return {
      kind: 'markdown',
      value: ts.displayPartsToString(item.documentation) + (item.tags ? '\n\n' + item.tags.map(tag => `_@${tag.name}_ — \`${ts.displayPartsToString(tag.text?.slice(0, 1))}\` ${ts.displayPartsToString(tag.text?.slice(1))}\n`).join('\n') : '')
    }
  }

  function convertTextSpanToRange(textSpan: ts.TextSpan, fileName: string): lsp.Range {
    const document = documentManager.getDocument(fileName)!;
    const start = document.positionAt(textSpan.start);
    const end = document.positionAt(textSpan.start + textSpan.length);

    return lsp.Range.create(start, end);
  }

  function formatQuickInfo(quickInfo: ts.QuickInfo, fileName: string): lsp.Hover {
    const contents = generateMarkdown(quickInfo)

    contents.value = ts.displayPartsToString(quickInfo.displayParts) + '\n\n' + contents.value

    // Convert the text span to an LSP range
    const range = convertTextSpanToRange(quickInfo.textSpan, fileName);

    return {
      contents,
      range
    };
  }
  // Assuming you have an initialized LSP connection and a TypeScript language service
  connection.onHover((params: lsp.TextDocumentPositionParams): lsp.Hover | null => {
    if (!params.textDocument.uri.endsWith(".vue")) return null;
    const virtualUrl = params.textDocument.uri.replace('file:', 'verter-virtual:') + '.tsx'
    let doc = documentManager.getDocument(virtualUrl);


    if (doc && 'template' in doc) {
      const templateInfo = doc.template
      const content = doc.getText()

      const originalIndex = offsetAt(params.position, content)
      const templatePos = templateInfo.mapConsumer.generatedPositionFor({
        column: params.position.character,
        line: params.position.line + 1,
        source: '.'

      })

      console.log('getting pos', originalIndex)

      const index = offsetAt({
        character: templatePos.column,
        line: templatePos.line - 1
      }, templateInfo.content)

      const quickInfo = tsService.getQuickInfoAtPosition(virtualUrl, index);
      if (!quickInfo) {
        return null;
      }
      return formatQuickInfo(quickInfo, virtualUrl);

      // let newIndex = params.context?.triggerCharacter === '@' || content[originalIndex - 1] === '@' ? index - 1 : index


      // }

      // // const { textDocument, position } = params;

      // // Convert LSP position to TypeScript position
      // const fileName = textDocument.uri;
      // const offset = document.offsetAt(position);

      // // Get quick info from TypeScript for the given position
      // const quickInfo = languageService.getQuickInfoAtPosition(fileName, offset);

      // if (!quickInfo) {
      //   return null;
      // }

      // // Format the response
      // return formatQuickInfo(quickInfo, fileName);
    }
    return null
  });


  connection.onCompletion(async (params) => {
    if (!params.textDocument.uri.endsWith(".vue")) return null;
    const virtualUrl = params.textDocument.uri.replace('file:', 'verter-virtual:') + '.tsx'
    const doc = documentManager.getDocument(virtualUrl);

    if (doc && 'template' in doc) {
      const templateInfo = doc.template
      const content = doc.getText()

      const originalIndex = offsetAt(params.position, content)
      const templatePos = templateInfo.mapConsumer.generatedPositionFor({
        column: params.position.character,
        line: params.position.line + 1,
        source: '.'

      })

      console.log('getting pos', originalIndex)

      const index = offsetAt({
        character: templatePos.column,
        line: templatePos.line - 1
      }, templateInfo.content)


      let newIndex = params.context?.triggerCharacter === '@' || content[originalIndex - 1] === '@' ? index - 1 : index

      try {
        const results = tsService.getCompletionsAtPosition(virtualUrl, newIndex, {
          triggerKind: params.context?.triggerKind,
          // @ts-expect-error this is correct, I think
          triggerCharacter: params.context?.triggerCharacter === '@' ? '' : params.context?.triggerCharacter,
        })

        const b = tsService.getQuickInfoAtPosition(virtualUrl, newIndex)
        const c = tsService.getTypeDefinitionAtPosition(virtualUrl, newIndex)
        const a = tsService.getReferencesAtPosition(virtualUrl, newIndex)

        // const otherResults = tsService.getCompletionEntryDetails(virtualUrl, newIndex, 'div', undefined, '<div', undefined, undefined)
        // const otherResults1 = tsService.getCompletionEntrySymbol(virtualUrl, newIndex, 'div', '<div')
        const rrr = tsService.getDefinitionAtPosition(virtualUrl, newIndex)
        const aa = tsService.getReferencesAtPosition(virtualUrl, newIndex)
        console.log('got res', newIndex, `'${templateInfo.content.slice(index - 5, index + 5)}'`, results?.entries.length)
        console.log({
          a, b, c, rrr, aa
        })
        // return results?.entries ?? []
        if (results) {

          // return results
          return {
            // ...results,
            isIncomplete: results.isIncomplete,

            items: results.entries.filter(x => {
              switch (params.context?.triggerCharacter) {
                case '@': {
                  return x.name.startsWith('on')
                }
                // case ':': 
              }

              return true

            }).flatMap(x => mapTsCompletionToLspItem(x, {
              virtualUrl,
              index: index,
              triggerKind: params.context?.triggerKind,
              triggerCharacter: params.context?.triggerCharacter,
            }))
          }
        }
      } catch (e) {
        console.error('eeee', e)
      }

      return {

        isIncomplete: false,
        items: [
          // {
          //   label: "hello",
          //   kind: lsp.CompletionItemKind.Class,
          //   data: 1,
          // },
        ],
      };
    }



    // const r = tsService.getCompletionsAtPosition(
    //   decodeURIComponent(
    //     params.textDocument.uri
    //       .replace("Test.vue", "Test.my.ts")
    //       .replace("file:///", "")
    //   ),
    //   params.position,
    //   { triggerKind: 1, triggerCharacter: "." }
    // );

    // debugger;
    // return r;

    return {
      isIncomplete: false,
      items: [
        {
          label: "hello",
          kind: lsp.CompletionItemKind.Class,
          data: 1,
        },
      ],
    };
  });

  console.log("should be listening now...");

  documentManager.listen(connection);

  connection.listen();
}
startServer();
