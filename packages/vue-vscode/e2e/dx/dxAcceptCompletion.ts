/**
 * Real completion-accept path for the extension-host DX driver.
 *
 * The auto-import accept gate must exercise the actual VS Code accept command path,
 * not a manual `WorkspaceEdit`. Existing helpers only call
 * `vscode.executeCompletionItemProvider`, which queries the provider without
 * applying the chosen item's text edits or its `additionalTextEdits` (the import).
 * `acceptCompletion` instead invokes `editor.action.triggerSuggest` then
 * `acceptSelectedSuggestion`, and asserts that BOTH the expected identifier landed
 * outside the static imports AND the static imports changed — the user-visible
 * accept the differential cares about.
 *
 * Both halves of the interaction finish AFTER their command returns, so the driver
 * synchronizes on observed editor state, never on elapsed time:
 *
 * - `triggerSuggest` only starts the suggestion widget's own completion request.
 *   An accept issued before the widget is showing a focused item is a silent no-op
 *   (its precondition is `suggestWidgetVisible && suggestWidgetHasFocusedSuggestion`).
 *   The accept is issued once the widget has focused the expected suggestion.
 * - An item whose import arrives through `completionItem/resolve` is inserted first
 *   and its additional (import) edit is applied later, when the resolve answers.
 *   The verdict is taken when the document reaches the accepted state.
 *
 * One trigger and one accept are issued, never more: re-triggering or re-accepting
 * until something happens would hide exactly the failure this gate exists to
 * catch. The injected deadline only bounds a hang; it never makes a pass.
 *
 * The command runner, text readers, observations and deadline are injected, so the
 * sequencing and the pass/fail verdict are unit-testable without VS Code.
 * {@link vscodeAcceptCompletionDeps} binds them to a type-only `vscode` API slice:
 * the suggestion widget's focused item is observed through the public inline
 * completion context (`InlineCompletionContext.selectedCompletionInfo`, re-requested
 * by VS Code whenever the widget's selection changes), and edits through
 * `workspace.onDidChangeTextDocument`.
 */

import ts from "typescript";
import type * as vscode from "vscode";

/** The VS Code command that opens the suggestion widget. */
export const TRIGGER_SUGGEST_COMMAND = "editor.action.triggerSuggest";
/** The VS Code command that accepts the selected suggestion (applies its edits). */
export const ACCEPT_SUGGESTION_COMMAND = "acceptSelectedSuggestion";

/** Before/after snapshots used to classify an accept. */
export interface AcceptSnapshot {
  readonly docBefore: string;
  readonly docAfter: string;
  readonly importBefore: string;
  readonly importAfter: string;
}

/** Classification of an accept attempt. */
export interface AcceptClassification {
  /**
   * The expected identifier was inserted: the document, with its static import
   * declarations excluded, holds more whole-identifier occurrences of the expected
   * suggestion than before the accept. An import-only edit cannot set this.
   */
  readonly identifierChanged: boolean;
  /** The static import declarations changed. */
  readonly importChanged: boolean;
  /** A real auto-import accept inserted the identifier AND changed the imports. */
  readonly accepted: boolean;
}

/**
 * Classify an accept: it counts as a real auto-import accept only when the
 * expected identifier landed outside the static imports AND the static imports
 * changed. Judging the identifier on the whole document would let an accept that
 * applied only its additional (import) edit pass, because that edit changes the
 * document too.
 */
export function classifyAcceptOutcome(
  snap: AcceptSnapshot,
  expectedSuggestion: string,
): AcceptClassification {
  const identifierChanged =
    countIdentifier(textOutsideStaticImports(snap.docAfter), expectedSuggestion) >
    countIdentifier(textOutsideStaticImports(snap.docBefore), expectedSuggestion);
  const importChanged = snap.importAfter !== snap.importBefore;
  return { identifierChanged, importChanged, accepted: identifierChanged && importChanged };
}

const IDENTIFIER_CHAR = /[\p{ID_Continue}$\u200c\u200d]/u;

/**
 * Count occurrences of `identifier` in `text` that are not part of a longer
 * identifier, so accepting `computedAsync` is not counted as `computed`.
 */
function countIdentifier(text: string, identifier: string): number {
  if (identifier.length === 0) return 0;
  let count = 0;
  for (let at = text.indexOf(identifier); at >= 0; at = text.indexOf(identifier, at + 1)) {
    const before = at > 0 ? text[at - 1] : "";
    const after = text[at + identifier.length] ?? "";
    if (!IDENTIFIER_CHAR.test(before) && !IDENTIFIER_CHAR.test(after)) count += 1;
  }
  return count;
}

/** The text of the `<script>` block (where an auto-import lands), or the whole doc. */
export function scriptBlockText(doc: { getText(): string }): string {
  const text = doc.getText();
  const start = text.indexOf("<script");
  const end = text.indexOf("</script>");
  return start >= 0 && end > start ? text.slice(start, end) : text;
}

/**
 * Parse the script content of a script/SFC-script snapshot and return its static
 * import (and import-equals) declarations with the offset of the parsed content.
 */
function parseStaticImports(scriptSnapshot: string): {
  readonly sourceFile: ts.SourceFile;
  readonly offset: number;
  readonly imports: Array<ts.ImportDeclaration | ts.ImportEqualsDeclaration>;
} {
  const offset = /^\s*<script\b[^>]*>/i.exec(scriptSnapshot)?.[0].length ?? 0;
  const sourceFile = ts.createSourceFile(
    "completion-accept.ts",
    scriptSnapshot.slice(offset),
    ts.ScriptTarget.Latest,
    true,
    ts.ScriptKind.TS,
  );
  const imports = sourceFile.statements.filter(
    (statement): statement is ts.ImportDeclaration | ts.ImportEqualsDeclaration =>
      ts.isImportDeclaration(statement) || ts.isImportEqualsDeclaration(statement),
  );
  return { sourceFile, offset, imports };
}

/**
 * The document text with the static import declarations of its `<script>` block
 * (see {@link scriptBlockText}) removed: where an accepted identifier must land.
 */
export function textOutsideStaticImports(docText: string): string {
  const start = docText.indexOf("<script");
  const end = docText.indexOf("</script>");
  const hasBlock = start >= 0 && end > start;
  const scriptStart = hasBlock ? start : 0;
  const scriptEnd = hasBlock ? end : docText.length;
  const { sourceFile, offset, imports } = parseStaticImports(docText.slice(scriptStart, scriptEnd));
  const kept: string[] = [];
  let cursor = scriptStart;
  for (const statement of imports) {
    kept.push(docText.slice(cursor, scriptStart + offset + statement.getStart(sourceFile)));
    cursor = scriptStart + offset + statement.getEnd();
  }
  kept.push(docText.slice(cursor));
  return docText.slice(0, scriptStart) + kept.join("");
}

/**
 * Extract only static import declarations from a script/SFC-script snapshot.
 *
 * The acceptance oracle must not use the whole `<script>` block as its import
 * fingerprint: accepting `comput` -> `computed` changes that block even when
 * the completion's additional import edit was dropped. TypeScript's parser is
 * used here so multiline, type-only, side-effect, and import-equals forms are
 * handled without a regex grammar.
 */
export function extractStaticImportText(scriptSnapshot: string): string {
  const { sourceFile, imports } = parseStaticImports(scriptSnapshot);
  return imports.map((statement) => statement.getText(sourceFile)).join("\n");
}

/** A subscription or armed timer the accept driver owns and releases. */
export interface AcceptSubscription {
  dispose(): void;
}

/** Injected dependencies for {@link acceptCompletion}. */
export interface AcceptCompletionDeps {
  /**
   * The text the expected suggestion inserts. The accept is issued only once the
   * visible suggestion widget has focused this item, so a different first
   * suggestion or a widget that never populated cannot be accepted; the accept
   * passes only once this identifier has landed outside the static imports.
   */
  readonly expectedSuggestion: string;
  /** Runs a VS Code command (the real path: `vscode.commands.executeCommand`). */
  runCommand: (command: string) => unknown | PromiseLike<unknown>;
  /** Reads the current document text (the whole document, imports included). */
  readDocText: () => string;
  /** Reads the current import-region text. */
  readImportText: () => string;
  /**
   * Observes the suggestion widget focusing an item; `listener` receives the text
   * the focused item would insert. Fires again whenever the focused item changes.
   */
  observeFocusedSuggestion: (listener: (insertText: string) => void) => AcceptSubscription;
  /** Observes a content change of the document the accept edits. */
  observeDocumentChange: (listener: () => void) => AcceptSubscription;
  /**
   * Arms the hang guard: `onExpired` runs once if the interaction has not
   * finished first. Expiry only ever produces a failure.
   */
  armDeadline: (onExpired: () => void) => AcceptSubscription;
}

/** The result of a successful {@link acceptCompletion}. */
export interface AcceptOutcome extends AcceptClassification, AcceptSnapshot {
  /** The commands invoked, in order. */
  readonly commands: string[];
}

/** A one-shot latch that a listener opens and the driver awaits. */
function latch(): { readonly opened: Promise<void>; open(): void } {
  let open!: () => void;
  const opened = new Promise<void>((resolve) => {
    open = resolve;
  });
  return { opened, open };
}

/**
 * Drive the real accept path: snapshot, `triggerSuggest`, wait until the suggestion
 * widget focuses {@link AcceptCompletionDeps.expectedSuggestion}, `acceptSelectedSuggestion`,
 * then wait until the expected identifier has landed outside the static imports AND
 * the static imports have changed. Each
 * command is issued exactly once. Throws if the widget never focuses the expected
 * item or the accept never reaches the accepted state before the deadline, and
 * releases every subscription on every ending. Returns the outcome on success.
 */
export async function acceptCompletion(deps: AcceptCompletionDeps): Promise<AcceptOutcome> {
  const commands: string[] = [];
  const docBefore = deps.readDocText();
  const importBefore = deps.readImportText();
  const observe = (): AcceptSnapshot => ({
    docBefore,
    docAfter: deps.readDocText(),
    importBefore,
    importAfter: deps.readImportText(),
  });

  const subscriptions: AcceptSubscription[] = [];
  const focusedTexts: string[] = [];
  const expectedFocused = latch();
  const acceptedState = latch();
  let triggerIssued = false;
  let acceptIssued = false;

  const checkAccepted = (): void => {
    if (acceptIssued && classifyAcceptOutcome(observe(), deps.expectedSuggestion).accepted) {
      acceptedState.open();
    }
  };

  const verdict = (snap: AcceptSnapshot): string => {
    const { identifierChanged, importChanged } = classifyAcceptOutcome(
      snap,
      deps.expectedSuggestion,
    );
    return `identifier changed=${identifierChanged}, import changed=${importChanged}`;
  };

  try {
    let expire!: (error: Error) => void;
    const expired = new Promise<never>((_, reject) => {
      expire = reject;
    });
    // The rejection is consumed by whichever wait is pending when it fires; mark it
    // handled so an expiry between waits is not reported as unhandled.
    expired.catch(() => undefined);
    subscriptions.push(
      deps.armDeadline(() =>
        expire(
          new Error(
            acceptIssued
              ? `acceptCompletion: real accept path did not complete the auto-import — ` +
                  verdict(observe())
              : `acceptCompletion: the suggestion widget never focused ` +
                  `${JSON.stringify(deps.expectedSuggestion)}, so no accept was issued — ` +
                  `focused=${JSON.stringify(focusedTexts)}, ${verdict(observe())}`,
          ),
        ),
      ),
    );
    const within = <T>(work: PromiseLike<T> | T): Promise<T> =>
      Promise.race([Promise.resolve(work), expired]);

    // Subscribe before issuing any command so no focus or edit is missed.
    subscriptions.push(
      deps.observeFocusedSuggestion((insertText) => {
        if (!triggerIssued || acceptIssued) return;
        focusedTexts.push(insertText);
        if (insertText === deps.expectedSuggestion) expectedFocused.open();
      }),
    );
    subscriptions.push(deps.observeDocumentChange(checkAccepted));

    triggerIssued = true;
    commands.push(TRIGGER_SUGGEST_COMMAND);
    await within(deps.runCommand(TRIGGER_SUGGEST_COMMAND));
    await within(expectedFocused.opened);

    acceptIssued = true;
    commands.push(ACCEPT_SUGGESTION_COMMAND);
    await within(deps.runCommand(ACCEPT_SUGGESTION_COMMAND));
    checkAccepted();
    await within(acceptedState.opened);

    const snap = observe();
    return { ...classifyAcceptOutcome(snap, deps.expectedSuggestion), ...snap, commands };
  } finally {
    for (const subscription of subscriptions.splice(0).reverse()) subscription.dispose();
  }
}

/** The slice of the `vscode` API the accept adapter drives. */
export interface VsCodeAcceptApi {
  readonly commands: { executeCommand(command: string): Thenable<unknown> };
  readonly workspace: {
    onDidChangeTextDocument(
      listener: (event: vscode.TextDocumentChangeEvent) => unknown,
    ): vscode.Disposable;
  };
  readonly languages: {
    registerInlineCompletionItemProvider(
      selector: vscode.DocumentSelector,
      provider: vscode.InlineCompletionItemProvider,
    ): vscode.Disposable;
  };
}

/** What the VS Code adapter needs besides the editor. */
export interface VsCodeAcceptOptions {
  /** See {@link AcceptCompletionDeps.expectedSuggestion}. */
  readonly expectedSuggestion: string;
  /** See {@link AcceptCompletionDeps.armDeadline}. */
  readonly armDeadline: AcceptCompletionDeps["armDeadline"];
}

/**
 * Bind the accept driver to a VS Code editor.
 *
 * Commands run through `executeCommand`; the document text is the whole document
 * (the identifier verdict excludes its static imports);
 * the import fingerprint contains only parsed static import declarations, so a
 * completion that changes an identifier without applying its additional import
 * edit cannot produce a false positive. The suggestion widget's focused item is
 * observed by an inline completion provider that contributes nothing: VS Code
 * requests inline completions with `selectedCompletionInfo` set whenever the
 * visible widget's selection changes, which is the editor's own report that the
 * list has loaded and an item is focused.
 */
export function vscodeAcceptCompletionDeps(
  api: VsCodeAcceptApi,
  editor: vscode.TextEditor,
  options: VsCodeAcceptOptions,
): AcceptCompletionDeps {
  const document = editor.document;
  const target = document.uri.toString();
  return {
    expectedSuggestion: options.expectedSuggestion,
    runCommand: (command) => api.commands.executeCommand(command),
    readDocText: () => document.getText(),
    readImportText: () => extractStaticImportText(scriptBlockText(document)),
    observeFocusedSuggestion: (listener) =>
      api.languages.registerInlineCompletionItemProvider(
        { language: document.languageId, scheme: document.uri.scheme },
        {
          provideInlineCompletionItems(candidate, _position, context) {
            const selected = context.selectedCompletionInfo;
            if (selected && candidate.uri.toString() === target) listener(selected.text);
            return undefined;
          },
        },
      ),
    observeDocumentChange: (listener) =>
      api.workspace.onDidChangeTextDocument((event) => {
        if (event.document.uri.toString() === target) listener();
      }),
    armDeadline: options.armDeadline,
  };
}
