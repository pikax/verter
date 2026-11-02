/**
 * Real completion-accept path for the extension-host DX driver.
 *
 * The auto-import accept gate must exercise the actual VS Code accept command path,
 * not a manual `WorkspaceEdit`. Existing helpers only call
 * `vscode.executeCompletionItemProvider`, which queries the provider without
 * applying the chosen item's text edits or its `additionalTextEdits` (the import).
 * `acceptCompletion` instead invokes `editor.action.triggerSuggest` then
 * `acceptSelectedSuggestion`, and asserts that BOTH the document text and the
 * import text changed — the user-visible accept the differential cares about.
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
  readonly docChanged: boolean;
  readonly importChanged: boolean;
  /** A real auto-import accept changed BOTH the document and the import text. */
  readonly accepted: boolean;
}

/**
 * Classify an accept: it counts as a real auto-import accept only when BOTH the
 * document text and the import text changed.
 */
export function classifyAcceptOutcome(snap: AcceptSnapshot): AcceptClassification {
  const docChanged = snap.docAfter !== snap.docBefore;
  const importChanged = snap.importAfter !== snap.importBefore;
  return { docChanged, importChanged, accepted: docChanged && importChanged };
}

/** The text of the `<script>` block (where an auto-import lands), or the whole doc. */
export function scriptBlockText(doc: { getText(): string }): string {
  const text = doc.getText();
  const start = text.indexOf("<script");
  const end = text.indexOf("</script>");
  return start >= 0 && end > start ? text.slice(start, end) : text;
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
  const openTagEnd = /^\s*<script\b[^>]*>/i.exec(scriptSnapshot)?.[0].length;
  const scriptText = openTagEnd === undefined ? scriptSnapshot : scriptSnapshot.slice(openTagEnd);
  const sourceFile = ts.createSourceFile(
    "completion-accept.ts",
    scriptText,
    ts.ScriptTarget.Latest,
    true,
    ts.ScriptKind.TS,
  );
  return sourceFile.statements
    .filter(
      (statement): statement is ts.ImportDeclaration | ts.ImportEqualsDeclaration =>
        ts.isImportDeclaration(statement) || ts.isImportEqualsDeclaration(statement),
    )
    .map((statement) => statement.getText(sourceFile))
    .join("\n");
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
   * suggestion or a widget that never populated cannot be accepted.
   */
  readonly expectedSuggestion: string;
  /** Runs a VS Code command (the real path: `vscode.commands.executeCommand`). */
  runCommand: (command: string) => unknown | PromiseLike<unknown>;
  /** Reads the current document text. */
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
 * then wait until the document AND its static imports have both changed. Each
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
    if (acceptIssued && classifyAcceptOutcome(observe()).accepted) acceptedState.open();
  };

  const verdict = (snap: AcceptSnapshot): string => {
    const { docChanged, importChanged } = classifyAcceptOutcome(snap);
    return `document changed=${docChanged}, import changed=${importChanged}`;
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
    return { ...classifyAcceptOutcome(snap), ...snap, commands };
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
 * Commands run through `executeCommand`; the document text is the whole document;
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
