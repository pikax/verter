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
 * The command runner / text readers are injected, so the sequencing and the
 * pass/fail verdict are unit-testable without VS Code. The in-host runner wires the
 * deps to `vscode.commands.executeCommand` and the active editor's document.
 */

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

/** Injected dependencies for {@link acceptCompletion}. */
export interface AcceptCompletionDeps {
  /** Runs a VS Code command (the real path: `vscode.commands.executeCommand`). */
  runCommand: (command: string) => unknown | PromiseLike<unknown>;
  /** Reads the current document text. */
  readDocText: () => string;
  /** Reads the current import-region text. */
  readImportText: () => string;
  /** Optional settle hook awaited after each command (e.g. let the widget open). */
  settle?: () => void | PromiseLike<void>;
}

/** The result of a successful {@link acceptCompletion}. */
export interface AcceptOutcome extends AcceptClassification, AcceptSnapshot {
  /** The commands invoked, in order. */
  readonly commands: string[];
}

/**
 * Drive the real accept path: snapshot, `triggerSuggest`, settle, then
 * `acceptSelectedSuggestion`, settle, snapshot, classify. Throws if the accept did
 * not change both the document and the import text. Returns the outcome on success.
 */
export async function acceptCompletion(deps: AcceptCompletionDeps): Promise<AcceptOutcome> {
  const commands: string[] = [];
  const docBefore = deps.readDocText();
  const importBefore = deps.readImportText();

  const run = async (command: string): Promise<void> => {
    commands.push(command);
    await deps.runCommand(command);
    await deps.settle?.();
  };

  await run(TRIGGER_SUGGEST_COMMAND);
  await run(ACCEPT_SUGGESTION_COMMAND);

  const docAfter = deps.readDocText();
  const importAfter = deps.readImportText();
  const snap: AcceptSnapshot = { docBefore, docAfter, importBefore, importAfter };
  const classification = classifyAcceptOutcome(snap);

  if (!classification.accepted) {
    throw new Error(
      `acceptCompletion: real accept path did not complete the auto-import — ` +
        `document changed=${classification.docChanged}, import changed=${classification.importChanged}`,
    );
  }

  return { ...classification, ...snap, commands };
}
