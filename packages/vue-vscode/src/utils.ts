import { Position } from "vscode";

/**
 * Convert a UTF-16 offset (from Rust LSP via negotiated UTF-16 encoding)
 * to a VS Code Position.
 *
 * JS strings use UTF-16 code units natively, so the offset IS a JS string index.
 * We iterate through the string counting newlines to find line and column.
 */
export function utf16OffsetToPosition(source: string, offset: number): Position {
  const clamped = Math.min(offset, source.length);
  let line = 0;
  let lastNewline = -1;
  for (let i = 0; i < clamped; i++) {
    if (source.charCodeAt(i) === 10) {
      line++;
      lastNewline = i;
    }
  }
  return new Position(line, clamped - lastNewline - 1);
}
