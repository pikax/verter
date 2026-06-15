/**
 * The per-edit sampling loop substrate.
 *
 * A scenario's edit `script` is a list of {@link EditStep}s applied to a single
 * driver-local document buffer; a `burst` step types one character at a time, one
 * `didChange` per character, so the harness can sample a probe AFTER EVERY CHARACTER
 * (the DX-collapse visibility requirement). {@link EditBuffer} is a pure incremental
 * buffer that turns each step into one or more {@link Tick}s — each carrying the new
 * document version, the incremental change (as UTF-16 offsets, encoding-agnostic),
 * the post-edit cursor offset, and the shifted anchor offsets — and {@link
 * runEditScript} drives a per-tick callback.
 *
 * The buffer holds UTF-16 offsets only; converting an offset into an LSP position in
 * the server's negotiated encoding is the LIVE driver's job (it reuses the
 * `@verter/lsp-test-client` `DocumentPositions` seam). Keeping the buffer
 * encoding-agnostic makes the whole loop unit-testable without a server.
 */

import type { EditStep } from "../scenario/index.js";

/** An incremental document change: replace `[startOffset, endOffset)` (UTF-16) with `text`. */
export interface ContentChange {
  /** UTF-16 offset (in the pre-tick text) of the replaced region's start. */
  readonly startOffset: number;
  /** UTF-16 offset (in the pre-tick text) of the replaced region's exclusive end. */
  readonly endOffset: number;
  /** The replacement text. */
  readonly text: string;
}

/** One observable step of the edit loop: a single incremental change plus the resulting state. */
export interface Tick {
  /** Index of the edit-script operation that produced this tick. */
  readonly editStepIndex: number;
  /** 0-based index of this tick WITHIN its step (0 for a non-burst step). */
  readonly tickIndex: number;
  /** The document version after this tick (the open version is the buffer's start version). */
  readonly version: number;
  /** The full document text AFTER this tick's change. */
  readonly text: string;
  /** The full document text BEFORE this tick's change (for encoding-aware position conversion). */
  readonly previousText: string;
  /** The incremental change applied this tick. */
  readonly change: ContentChange;
  /** The UTF-16 offset of the sample/cursor position after the change. */
  readonly cursor: number;
  /** Every anchor's current UTF-16 offset after the change. */
  readonly anchors: Readonly<Record<string, number>>;
}

/**
 * A pure incremental edit buffer over a single document, tracking the live text,
 * version, and named anchor offsets. Each {@link applyStep} mutates the buffer and
 * returns the ticks the step produced (one for a non-burst edit, one per typed code
 * point for a burst). Anchor offsets shift with every change: an anchor at or before
 * a change start is unmoved, one at or after the change end shifts by the length
 * delta, and one strictly inside a replaced/deleted region clamps to the start.
 */
export class EditBuffer {
  private text_: string;
  private version_: number;
  private readonly anchors_: Map<string, number>;

  constructor(text: string, anchors: Readonly<Record<string, number>>, startVersion = 1) {
    this.text_ = text;
    this.version_ = startVersion;
    this.anchors_ = new Map(Object.entries(anchors));
  }

  get text(): string {
    return this.text_;
  }

  get version(): number {
    return this.version_;
  }

  /** The current UTF-16 offset of `anchor`. */
  anchorOffset(anchor: string): number {
    const offset = this.anchors_.get(anchor);
    if (offset === undefined) {
      throw new Error(`unknown anchor "${anchor}"`);
    }
    return offset;
  }

  /** A snapshot of every anchor's current offset. */
  anchorSnapshot(): Record<string, number> {
    return Object.fromEntries(this.anchors_);
  }

  /**
   * Apply one change: splice `[start, end)` → `text`, bump the version, shift every
   * anchor, and return the resulting tick. The cursor is placed just after the
   * inserted text.
   */
  private applyChange(
    editStepIndex: number,
    tickIndex: number,
    start: number,
    end: number,
    text: string,
  ): Tick {
    const previousText = this.text_;
    this.text_ = previousText.slice(0, start) + text + previousText.slice(end);
    this.version_ += 1;
    this.shiftAnchors(start, end, text.length);
    return {
      editStepIndex,
      tickIndex,
      version: this.version_,
      text: this.text_,
      previousText,
      change: { startOffset: start, endOffset: end, text },
      cursor: start + text.length,
      anchors: this.anchorSnapshot(),
    };
  }

  /** Shift every anchor for a `[start, end) → length`-unit replacement. */
  private shiftAnchors(start: number, end: number, insertedLength: number): void {
    const delta = insertedLength - (end - start);
    for (const [name, offset] of this.anchors_) {
      if (offset <= start) continue; // at or before the edit — unmoved
      if (offset >= end) {
        this.anchors_.set(name, offset + delta); // at or after the edit — shifted
      } else {
        this.anchors_.set(name, start); // strictly inside the replaced region — clamped
      }
    }
  }

  /**
   * Apply one {@link EditStep}, returning its ticks. A non-burst insert/replace is one
   * change; a burst types each code point as its own zero-width insert (so a surrogate
   * pair stays one tick); a burst replace deletes once (when there is anything to
   * remove) then types each code point; a delete is one change.
   */
  applyStep(step: EditStep, editStepIndex: number): Tick[] {
    const base = this.anchorOffset(step.anchor);
    const ticks: Tick[] = [];

    if (step.kind === "delete") {
      ticks.push(this.applyChange(editStepIndex, 0, base, base + step.removeUnits, ""));
      return ticks;
    }

    // insert | replace. A replace first removes `removeUnits` at the anchor.
    const removeUnits = step.kind === "replace" ? step.removeUnits : 0;

    if (!step.burst) {
      // One combined change: replace the removed span (if any) with the whole text.
      ticks.push(this.applyChange(editStepIndex, 0, base, base + removeUnits, step.text));
      return ticks;
    }

    // Burst: an optional single removal tick, then one tick per typed code point.
    let tickIndex = 0;
    if (removeUnits > 0) {
      ticks.push(this.applyChange(editStepIndex, tickIndex++, base, base + removeUnits, ""));
    }
    // Spreading the string iterates by code point, so a surrogate pair is one tick.
    let insertAt = base;
    for (const codePoint of step.text) {
      ticks.push(this.applyChange(editStepIndex, tickIndex++, insertAt, insertAt, codePoint));
      insertAt += codePoint.length; // advance by the code point's UTF-16 unit count
    }
    return ticks;
  }
}

/**
 * Drive `onTick` once per tick across every step of `script`, in script order,
 * awaiting an async callback before advancing. The buffer is mutated in place; after
 * the run it holds the final document state.
 */
export async function runEditScript(
  buffer: EditBuffer,
  script: readonly EditStep[],
  onTick: (tick: Tick) => void | Promise<void>,
): Promise<void> {
  for (let i = 0; i < script.length; i++) {
    const ticks = buffer.applyStep(script[i], i);
    for (const tick of ticks) {
      await onTick(tick);
    }
  }
}
