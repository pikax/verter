import { mkdtempSync, readFileSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";

import { describe, expect, it } from "vitest";

import { collectorEvent, type CollectorEvent, type CollectorEventKey } from "../src/index.js";
import {
  DX_EVENTS_FILENAME,
  JsonlEventSink,
  parseEvents,
  readEventsJsonl,
  serializeEvents,
  writeEventsJsonl,
} from "../src/report/index.js";

function key(overrides: Partial<CollectorEventKey> = {}): CollectorEventKey {
  return {
    scenario: "completion-member-access",
    editStepIndex: 3,
    driver: "rawLsp",
    provider: "tsgo-rust",
    probe: "p-complete",
    version: 7,
    anchor: "cursor",
    ...overrides,
  };
}

/** A representative spread: an event WITH `data`, one WITHOUT, and one whose detail/data
 *  carry newlines and unicode (the JSONL line must never break across records). */
function sampleEvents(): CollectorEvent[] {
  return [
    collectorEvent({
      collector: "completion",
      signal: "no_suggestions_collapse",
      ok: false,
      severity: "candidate",
      provenance: {
        detectedBy: "rawLsp",
        confirmedBy: "extensionHost",
        escalatesTo: "userVisible",
      },
      key: key(),
      detail: "verter returned no completions mid-typing",
      data: { mutation: "insertion", baselineLabelCount: 12 },
    }),
    collectorEvent({
      collector: "hover",
      signal: "hover_observed",
      ok: true,
      severity: "userVisible",
      provenance: { detectedBy: "rawLsp" },
      key: key({ probe: "p-hover", anchor: "ident", editStepIndex: -1 }),
      detail: "verter produced a hover type label",
    }),
    collectorEvent({
      collector: "logs",
      signal: "server_warn",
      ok: false,
      severity: "candidate",
      provenance: { detectedBy: "rawLsp" },
      key: key({ probe: "p-log" }),
      detail: "server logged a warning:\r\nWARN something\nwith a façade",
      data: { line: "WARN something" },
    }),
  ];
}

describe("report/events — JSONL serialization", () => {
  it("serializes one event per line with no embedded record-breaking newline", () => {
    const events = sampleEvents();
    const text = serializeEvents(events);
    const lines = text.split("\n").filter((l) => l.length > 0);
    expect(lines).toHaveLength(events.length);
    // Each line is a self-contained JSON object — no literal newline leaked from `detail`.
    for (const line of lines) {
      const parsed = JSON.parse(line) as { collector: string };
      expect(typeof parsed.collector).toBe("string");
    }
  });

  it("emits a stable top-level field order (collector, signal, ok, severity, provenance, key, detail, data)", () => {
    const [withData] = sampleEvents();
    const line = serializeEvents([withData]).trim();
    expect(line.startsWith('{"collector":')).toBe(true);
    const keysInOrder = [
      ...line.matchAll(/"(collector|signal|ok|severity|provenance|key|detail|data)":/g),
    ].map((m) => m[1]);
    expect(keysInOrder).toEqual([
      "collector",
      "signal",
      "ok",
      "severity",
      "provenance",
      "key",
      "detail",
      "data",
    ]);
  });

  it("round-trips: parse(serialize(events)) deep-equals the originals, omitting absent data", () => {
    const events = sampleEvents();
    const restored = parseEvents(serializeEvents(events));
    expect(restored).toEqual(events);
    // the data-less event must NOT gain a `data` key on the way back.
    expect("data" in restored[1]).toBe(false);
  });

  it("tolerates blank trailing lines and a missing trailing newline", () => {
    const events = sampleEvents();
    const withoutNewline = serializeEvents(events, { trailingNewline: false });
    const withBlankTail = `${serializeEvents(events)}\n\n  \n`;
    expect(parseEvents(withoutNewline)).toEqual(events);
    expect(parseEvents(withBlankTail)).toEqual(events);
  });

  it("rejects a malformed JSONL line with a clear, line-numbered error", () => {
    // A valid first line then a non-JSON second line: the error names line 2.
    const validFirst = serializeEvents([sampleEvents()[1]], { trailingNewline: false });
    expect(() => parseEvents(`${validFirst}\nnot-json`)).toThrow(/line 2/);
    // a structurally-incomplete event (no key) is rejected, not silently coerced.
    expect(() => parseEvents('{"collector":"completion","signal":"completion_parity"}')).toThrow(
      /line 1/,
    );
  });

  it("rejects a structurally-valid event whose signal/collector is outside the closed taxonomy", () => {
    const valid = JSON.parse(
      serializeEvents([sampleEvents()[1]], { trailingNewline: false }),
    ) as Record<string, unknown>;
    // an unregistered SIGNAL (every other field well-formed) is rejected at the boundary.
    expect(() =>
      parseEvents(JSON.stringify({ ...valid, signal: "totally_made_up_signal" })),
    ).toThrow(/line 1/);
    // an unregistered COLLECTOR is rejected too.
    expect(() => parseEvents(JSON.stringify({ ...valid, collector: "not_a_collector" }))).toThrow(
      /line 1/,
    );
    // the unmodified, registered event still round-trips.
    expect(parseEvents(JSON.stringify(valid))).toHaveLength(1);
  });
});

describe("report/events — file sink + IO", () => {
  it("JsonlEventSink buffers emitted events and serializes identically to serializeEvents", () => {
    const events = sampleEvents();
    const sink = new JsonlEventSink();
    for (const event of events) sink.emit(event);
    expect(sink.events).toEqual(events);
    expect(sink.serialize()).toBe(serializeEvents(events));
  });

  it("writes and reads back a dx-events.jsonl file losslessly", () => {
    const dir = mkdtempSync(join(tmpdir(), "dx-events-"));
    try {
      const events = sampleEvents();
      const file = join(dir, DX_EVENTS_FILENAME);
      writeEventsJsonl(file, events);
      const onDisk = readFileSync(file, "utf8");
      expect(onDisk.endsWith("\n")).toBe(true);
      expect(readEventsJsonl(file)).toEqual(events);
    } finally {
      rmSync(dir, { recursive: true, force: true });
    }
  });
});
