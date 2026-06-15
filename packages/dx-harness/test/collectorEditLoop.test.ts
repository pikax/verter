import { describe, expect, it } from "vitest";

import { EditBuffer, runEditScript, type Tick } from "../src/collectors/index.js";
import type { EditStep } from "../src/index.js";

describe("EditBuffer — incremental ticks, version bumps, cursor + anchor tracking", () => {
  it("a non-burst insert produces ONE tick with the whole text and one version bump", () => {
    const buf = new EditBuffer("ab", { mid: 1 }, 1);
    const ticks = buf.applyStep({ kind: "insert", anchor: "mid", text: "XY" }, 0);
    expect(ticks).toHaveLength(1);
    expect(ticks[0].version).toBe(2);
    expect(ticks[0].text).toBe("aXYb");
    expect(ticks[0].change).toEqual({ startOffset: 1, endOffset: 1, text: "XY" });
    expect(ticks[0].cursor).toBe(3); // just after the inserted "XY"
    expect(buf.text).toBe("aXYb");
    expect(buf.version).toBe(2);
  });

  it("a BURST insert produces one tick PER character, each a zero-width insert, cursor advancing", () => {
    const buf = new EditBuffer("ab", { start: 0, mid: 1, end: 2 }, 1);
    const ticks = buf.applyStep({ kind: "insert", anchor: "mid", text: "XY", burst: true }, 0);
    expect(ticks).toHaveLength(2);

    expect(ticks[0].version).toBe(2);
    expect(ticks[0].text).toBe("aXb");
    expect(ticks[0].previousText).toBe("ab");
    expect(ticks[0].change).toEqual({ startOffset: 1, endOffset: 1, text: "X" });
    expect(ticks[0].cursor).toBe(2);
    // The edited anchor STAYS at its offset; a later anchor shifts right by the insert.
    expect(ticks[0].anchors).toEqual({ start: 0, mid: 1, end: 3 });

    expect(ticks[1].version).toBe(3);
    expect(ticks[1].text).toBe("aXYb");
    expect(ticks[1].change).toEqual({ startOffset: 2, endOffset: 2, text: "Y" });
    expect(ticks[1].cursor).toBe(3);
    expect(ticks[1].anchors).toEqual({ start: 0, mid: 1, end: 4 });
  });

  it("a delete removes UTF-16 units, clamping interior anchors to the start and shifting trailing ones", () => {
    const buf = new EditBuffer("abcde", { head: 1, inside: 2, tail: 4 }, 1);
    const ticks = buf.applyStep({ kind: "delete", anchor: "head", removeUnits: 2 }, 0);
    expect(ticks).toHaveLength(1);
    expect(ticks[0].text).toBe("ade");
    expect(ticks[0].change).toEqual({ startOffset: 1, endOffset: 3, text: "" });
    expect(ticks[0].cursor).toBe(1);
    // head stays (at the deletion start); inside (2) clamps to start (1); tail (4) shifts -2 → 2.
    expect(ticks[0].anchors).toEqual({ head: 1, inside: 1, tail: 2 });
  });

  it("a non-burst replace replaces the range with the text in one change", () => {
    const buf = new EditBuffer("abcde", { p: 1 }, 1);
    const ticks = buf.applyStep({ kind: "replace", anchor: "p", removeUnits: 2, text: "XYZ" }, 0);
    expect(ticks).toHaveLength(1);
    expect(ticks[0].text).toBe("aXYZde");
    expect(ticks[0].change).toEqual({ startOffset: 1, endOffset: 3, text: "XYZ" });
    expect(ticks[0].cursor).toBe(4); // start + replacement length
  });

  it("a BURST replace deletes once then types each character", () => {
    const buf = new EditBuffer("abcde", { p: 1 }, 1);
    const ticks = buf.applyStep(
      { kind: "replace", anchor: "p", removeUnits: 2, text: "XY", burst: true },
      0,
    );
    expect(ticks).toHaveLength(3);
    expect(ticks[0].change).toEqual({ startOffset: 1, endOffset: 3, text: "" });
    expect(ticks[0].text).toBe("ade");
    expect(ticks[1].change).toEqual({ startOffset: 1, endOffset: 1, text: "X" });
    expect(ticks[2].change).toEqual({ startOffset: 2, endOffset: 2, text: "Y" });
    expect(ticks[2].text).toBe("aXYde");
    expect(ticks[2].cursor).toBe(3);
  });

  it("types a surrogate-pair code point as a SINGLE burst tick (not split across the pair)", () => {
    const buf = new EditBuffer("()", { c: 1 }, 1);
    // "😀" is one code point but two UTF-16 units.
    const ticks = buf.applyStep({ kind: "insert", anchor: "c", text: "😀", burst: true }, 0);
    expect(ticks).toHaveLength(1);
    expect(ticks[0].text).toBe("(😀)");
    expect(ticks[0].cursor).toBe(3); // advanced by the 2 UTF-16 units of the code point
  });

  it("throws on an edit referencing an unknown anchor", () => {
    const buf = new EditBuffer("ab", { mid: 1 }, 1);
    expect(() => buf.applyStep({ kind: "insert", anchor: "nope", text: "x" }, 0)).toThrow(/anchor/);
  });
});

describe("runEditScript — drives onTick per tick in order", () => {
  it("invokes onTick for every tick across every step, in script order", async () => {
    const buf = new EditBuffer("ab", { mid: 1 }, 1);
    const script: EditStep[] = [
      { kind: "insert", anchor: "mid", text: "XY", burst: true },
      { kind: "delete", anchor: "mid", removeUnits: 1 },
    ];
    const seen: Array<{ step: number; tick: number; version: number; cursor: number }> = [];
    await runEditScript(buf, script, (tick: Tick) => {
      seen.push({
        step: tick.editStepIndex,
        tick: tick.tickIndex,
        version: tick.version,
        cursor: tick.cursor,
      });
    });
    expect(seen).toEqual([
      { step: 0, tick: 0, version: 2, cursor: 2 },
      { step: 0, tick: 1, version: 3, cursor: 3 },
      { step: 1, tick: 0, version: 4, cursor: 1 },
    ]);
  });

  it("awaits an async onTick before advancing to the next tick", async () => {
    const buf = new EditBuffer("ab", { mid: 1 }, 1);
    const order: string[] = [];
    await runEditScript(
      buf,
      [{ kind: "insert", anchor: "mid", text: "AB", burst: true }],
      async (t) => {
        order.push(`enter:${t.tickIndex}`);
        await Promise.resolve();
        order.push(`exit:${t.tickIndex}`);
      },
    );
    expect(order).toEqual(["enter:0", "exit:0", "enter:1", "exit:1"]);
  });
});
