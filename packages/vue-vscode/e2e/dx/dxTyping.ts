/**
 * Incremental, character-by-character typing for the extension-host DX driver.
 *
 * Existing E2E mutates documents with coarse `WorkspaceEdit`s; DX needs the real
 * per-keystroke path so completion collapse and per-character churn are visible.
 * `typeChars` drives `editor.edit((b) => b.insert(...))` once per character, so each
 * keystroke is a distinct `textDocument/didChange` to the server.
 *
 * It is written purely against the INSTANCE methods of the injected editor/position
 * (`editor.edit`, `pos.translate`, `pos.with`) and uses a type-only `vscode` import,
 * so it carries no runtime dependency on the `vscode` module and is unit-testable
 * with in-memory fakes.
 */
import type * as vscode from "vscode";

/** A sample handed to the {@link TypeCharsSampler} after each inserted character. */
export interface TypeCharsSample {
  /** Zero-based index of the character within `text`. */
  readonly index: number;
  /** The character just inserted. */
  readonly char: string;
  /** The cursor position AFTER the insert (where the next character will go). */
  readonly position: vscode.Position;
}

/** Optional callback invoked after each character (e.g. to sample completions). */
export type TypeCharsSampler = (sample: TypeCharsSample) => void | Promise<void>;

/**
 * Type `text` into `editor` one character at a time starting at `start`, applying
 * one `editor.edit` insert per character and advancing the cursor across line
 * breaks. Awaits the optional `sampler` after each character. Returns the final
 * cursor position. Throws if any edit is rejected — a keystroke is never silently
 * dropped.
 */
export async function typeChars(
  editor: vscode.TextEditor,
  start: vscode.Position,
  text: string,
  sampler?: TypeCharsSampler,
): Promise<vscode.Position> {
  let position = start;
  let index = 0;
  for (const char of text) {
    const applied = await editor.edit((builder) => builder.insert(position, char));
    if (!applied) {
      throw new Error(
        `typeChars: editor.edit rejected insert of ${JSON.stringify(char)} at ` +
          `${position.line}:${position.character}`,
      );
    }
    position =
      char === "\n" ? position.with(position.line + 1, 0) : position.translate(0, char.length);
    await sampler?.({ index, char, position });
    index += 1;
  }
  return position;
}
