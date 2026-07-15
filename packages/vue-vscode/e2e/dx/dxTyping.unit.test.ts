import { describe, expect, it } from "vitest";

import { typeChars, type TypeCharsSample } from "./dxTyping";

/** Minimal in-memory stand-in for `vscode.Position` (instance methods only). */
class FakePosition {
  constructor(
    readonly line: number,
    readonly character: number,
  ) {}
  translate(lineDelta = 0, characterDelta = 0): FakePosition {
    return new FakePosition(this.line + lineDelta, this.character + characterDelta);
  }
  with(line: number = this.line, character: number = this.character): FakePosition {
    return new FakePosition(line, character);
  }
}

interface RecordedInsert {
  line: number;
  character: number;
  text: string;
}

/** Fake editor whose `edit` synchronously applies a single insert via the builder. */
function fakeEditor(opts: { applied?: boolean } = {}) {
  const inserts: RecordedInsert[] = [];
  const editor = {
    edit(cb: (b: { insert(p: FakePosition, t: string): void }) => void): Promise<boolean> {
      cb({
        insert(p: FakePosition, t: string) {
          inserts.push({ line: p.line, character: p.character, text: t });
        },
      });
      return Promise.resolve(opts.applied ?? true);
    },
  };
  return { editor, inserts };
}

describe("typeChars", () => {
  it("inserts one character per edit at advancing positions on a single line", async () => {
    const { editor, inserts } = fakeEditor();
    const start = new FakePosition(3, 5);
    // eslint-disable-next-line @typescript-eslint/no-explicit-any
    const end = await typeChars(editor as any, start as any, "abc");

    expect(inserts).toEqual([
      { line: 3, character: 5, text: "a" },
      { line: 3, character: 6, text: "b" },
      { line: 3, character: 7, text: "c" },
    ]);
    expect({
      line: (end as FakePosition).line,
      character: (end as FakePosition).character,
    }).toEqual({ line: 3, character: 8 });
  });

  it("advances to the next line on a newline character", async () => {
    const { editor, inserts } = fakeEditor();
    const start = new FakePosition(0, 0);
    // eslint-disable-next-line @typescript-eslint/no-explicit-any
    await typeChars(editor as any, start as any, "a\nb");
    expect(inserts).toEqual([
      { line: 0, character: 0, text: "a" },
      { line: 0, character: 1, text: "\n" },
      { line: 1, character: 0, text: "b" },
    ]);
  });

  it("invokes the sampler after each character with index, char and cursor position", async () => {
    const { editor } = fakeEditor();
    const samples: TypeCharsSample[] = [];
    const start = new FakePosition(0, 0);
    // eslint-disable-next-line @typescript-eslint/no-explicit-any
    await typeChars(editor as any, start as any, "hi", (s) => {
      samples.push(s);
    });
    expect(samples.map((s) => s.char)).toEqual(["h", "i"]);
    expect(samples.map((s) => s.index)).toEqual([0, 1]);
    // Position is the cursor AFTER the inserted character.
    expect((samples[0].position as unknown as FakePosition).character).toBe(1);
    expect((samples[1].position as unknown as FakePosition).character).toBe(2);
  });

  it("does nothing for empty text and returns the start position", async () => {
    const { editor, inserts } = fakeEditor();
    const start = new FakePosition(2, 2);
    // eslint-disable-next-line @typescript-eslint/no-explicit-any
    const end = await typeChars(editor as any, start as any, "");
    expect(inserts).toHaveLength(0);
    expect(end).toBe(start);
  });

  it("throws when an edit is rejected (no silent drop of a keystroke)", async () => {
    const { editor } = fakeEditor({ applied: false });
    const start = new FakePosition(0, 0);
    await expect(
      // eslint-disable-next-line @typescript-eslint/no-explicit-any
      typeChars(editor as any, start as any, "x"),
    ).rejects.toThrow(/edit/i);
  });
});
