import { describe, expect, it } from "vitest";

import { toNegotiatedPosition } from "./lsp-bench.position";

describe("toNegotiatedPosition (benchmark hover-position encoding conversion)", () => {
  it("shifts a UTF-16 config character to its utf-8 byte offset when non-ASCII precedes it", () => {
    // "café " is 5 UTF-16 code units but 6 UTF-8 bytes (é = U+00E9 → 2 bytes).
    // The benchmark config supplies a 1-based UTF-16 position that lsp-bench.config
    // makes 0-based; here the 0-based UTF-16 target is character 5 — immediately
    // after "café ".
    const text = "café xy\nsecond";
    const source = { line: 0, character: 5 };

    const sent = toNegotiatedPosition(text, source, "utf-8");

    // Under utf-8 the negotiated character is the BYTE offset (6), not the raw
    // UTF-16 code-unit count (5).
    expect(sent).toEqual({ line: 0, character: 6 });
    // Negative: a regression that sent the raw config character verbatim under
    // utf-8 would probe the wrong column — the converted offset must differ.
    expect(sent.character).not.toBe(source.character);
  });

  it("is the identity under utf-16 (the editor-native config encoding)", () => {
    const text = "café xy\nsecond";
    const source = { line: 0, character: 5 };

    const sent = toNegotiatedPosition(text, source, "utf-16");

    expect(sent).toEqual({ line: 0, character: 5 });
    expect(sent.character).toBe(source.character);
  });

  it("counts only the characters before the target on its own line", () => {
    // A non-ASCII run on line 1 (α, β each = 2 utf-8 bytes) before the target
    // proves the conversion is per-line, not a whole-document byte walk.
    const text = "first\nαβ z";
    const source = { line: 1, character: 3 }; // 0-based utf-16: just after "αβ "

    const sent = toNegotiatedPosition(text, source, "utf-8");

    // "αβ " = 2 + 2 + 1 = 5 utf-8 bytes on line 1.
    expect(sent).toEqual({ line: 1, character: 5 });
    expect(sent.character).not.toBe(source.character);
  });
});
