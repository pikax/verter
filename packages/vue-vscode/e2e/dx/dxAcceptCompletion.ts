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

import ts from "typescript";

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

/** Injected dependencies for {@link acceptCompletion}. */
export interface AcceptCompletionDeps {
  /** Runs a VS Code command (the real path: `vscode.commands.executeCommand`). */
  runCommand: (command: string) => unknown | PromiseLike<unknown>;
  /** Reads the current document text. */
  readDocText: () => string;
  /** Reads the current import-region text. */
  readImportText: () => string;
  /** Settle hook awaited after each command and between observations. */
  settle?: () => void | PromiseLike<void>;
}

/** The result of a successful {@link acceptCompletion}. */
export interface AcceptOutcome extends AcceptClassification, AcceptSnapshot {
  /** The commands invoked, in order. */
  readonly commands: string[];
}

/** Settles an accept may take, from the trigger to its import edit landing. */
const MAX_ACCEPT_SETTLES = 16;
/**
 * Accepts issued per trigger. Triggering again cancels a list still loading, so
 * it waits this long for a list that closed without offering anything.
 */
const ACCEPTS_PER_TRIGGER = 7;

/**
 * Drive the real accept path: snapshot, `triggerSuggest`, `acceptSelectedSuggestion`,
 * then observe the document and import text. VS Code reports neither the list
 * loading nor the import edit landing, and accepting before the list has loaded
 * does nothing, so the accept is issued until the document changes and the import
 * is then awaited. Throws if the document and the import text did not both change
 * within {@link MAX_ACCEPT_SETTLES}. Returns the outcome on success.
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

  const observe = (): AcceptSnapshot => ({
    docBefore,
    docAfter: deps.readDocText(),
    importBefore,
    importAfter: deps.readImportText(),
  });

  let acceptsSinceTrigger = ACCEPTS_PER_TRIGGER;
  let snap = observe();
  for (let settles = 0; settles < MAX_ACCEPT_SETTLES; ) {
    if (!classifyAcceptOutcome(snap).docChanged) {
      if (acceptsSinceTrigger === ACCEPTS_PER_TRIGGER) {
        await run(TRIGGER_SUGGEST_COMMAND);
        settles += 1;
        acceptsSinceTrigger = 0;
      }
      await run(ACCEPT_SUGGESTION_COMMAND);
      acceptsSinceTrigger += 1;
    } else {
      await deps.settle?.();
    }
    settles += 1;
    snap = observe();
    if (classifyAcceptOutcome(snap).accepted) break;
  }
  const classification = classifyAcceptOutcome(snap);

  if (!classification.accepted) {
    throw new Error(
      `acceptCompletion: real accept path did not complete the auto-import — ` +
        `document changed=${classification.docChanged}, import changed=${classification.importChanged}`,
    );
  }

  return { ...classification, ...snap, commands };
}
