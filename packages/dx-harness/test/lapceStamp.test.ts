/**
 * WSP1L stamp-channel parsers — the dx-harness side of the volt's stderr
 * instrumentation contract (`verter launch-stamp` / `verter ui-stamp`). Every
 * field is validated; malformed stamps throw loud and nothing is ever guessed.
 */
import { describe, expect, it } from "vitest";

import {
  LAUNCH_STAMP_MESSAGE_PREFIX,
  UI_STAMP_MESSAGE_PREFIX,
  collectStampMessages,
  isStampMessage,
  parseLaunchStampMessage,
  parseLaunchStampValue,
  parseUiStampMessage,
  parseUiStampValue,
  stampUnixMsToTimelineMs,
} from "../lapce/index.js";

const ISSUED_LINE =
  `${LAUNCH_STAMP_MESSAGE_PREFIX}` +
  `{"phase":"server_launch_issued","atUnixMs":1758000000000,"serverUri":"urn:/opt/verter/verter-lsp",` +
  `"workspaceRoot":"/home/dev/proj","documentLanguages":["vue","svelte"]}`;

const REFUSED_LINE =
  `${LAUNCH_STAMP_MESSAGE_PREFIX}` +
  `{"phase":"launch_refused","atUnixMs":42,"serverUri":"","workspaceRoot":"/x",` +
  `"documentLanguages":["vue","svelte"],"refusal":"no discovery source selected"}`;

const UI_LINE =
  `${UI_STAMP_MESSAGE_PREFIX}` +
  `{"kind":"type","requestEpoch":10,"sourceEpoch":null,"stage":"decoded","atUnixMs":1758000000037}`;

describe("launch-stamp parsing (the volt's initialize epoch marker)", () => {
  it("parses an issued launch stamp verbatim", () => {
    const stamp = parseLaunchStampMessage(ISSUED_LINE);
    expect(stamp.phase).toBe("server_launch_issued");
    expect(stamp.atUnixMs).toBe(1_758_000_000_000);
    expect(stamp.serverUri).toBe("urn:/opt/verter/verter-lsp");
    expect(stamp.workspaceRoot).toBe("/home/dev/proj");
    expect(stamp.documentLanguages).toEqual(["vue", "svelte"]);
    expect(stamp.refusal).toBeUndefined();
  });

  it("parses a refused launch stamp with its reason", () => {
    const stamp = parseLaunchStampMessage(REFUSED_LINE);
    expect(stamp.phase).toBe("launch_refused");
    expect(stamp.atUnixMs).toBe(42);
    expect(stamp.refusal).toBe("no discovery source selected");
  });

  it("throws loud on a non-stamp line, malformed JSON, or a non-object payload", () => {
    expect(() => parseLaunchStampMessage("lapce: some unrelated stderr noise")).toThrow(
      /not a 'verter launch-stamp' line/,
    );
    expect(() => parseLaunchStampMessage(`${LAUNCH_STAMP_MESSAGE_PREFIX}{nope`)).toThrow(
      /malformed verter launch-stamp JSON/,
    );
    expect(() => parseLaunchStampValue([1, 2])).toThrow(/must be an object/);
  });

  it("throws loud on wrong phases, non-integer clocks, and refusal inconsistency", () => {
    expect(() =>
      parseLaunchStampValue({
        phase: "smoke",
        atUnixMs: 1,
        serverUri: "",
        workspaceRoot: "",
        documentLanguages: [],
      }),
    ).toThrow(/phase must be/);
    expect(() =>
      parseLaunchStampValue({
        phase: "server_launch_issued",
        atUnixMs: 1.5,
        serverUri: "",
        workspaceRoot: "",
        documentLanguages: [],
      }),
    ).toThrow(/atUnixMs must be a non-negative integer/);
    expect(() =>
      parseLaunchStampValue({
        phase: "launch_refused",
        atUnixMs: 1,
        serverUri: "",
        workspaceRoot: "",
        documentLanguages: [],
      }),
    ).toThrow(/non-empty refusal/);
    expect(() =>
      parseLaunchStampValue({
        phase: "server_launch_issued",
        atUnixMs: 1,
        serverUri: "",
        workspaceRoot: "",
        documentLanguages: [],
        refusal: "stale refusal",
      }),
    ).toThrow(/must not carry a refusal/);
  });
});

describe("ui-stamp parsing (instrumented-client event-loop observations)", () => {
  it("parses a UI stage observation on the WSP1 epoch basis", () => {
    const stamp = parseUiStampMessage(UI_LINE);
    expect(stamp).toEqual({
      kind: "type",
      requestEpoch: 10,
      sourceEpoch: null,
      stage: "decoded",
      atUnixMs: 1_758_000_000_037,
    });
  });

  it("throws loud on unknown kinds, stages, epochs and clocks", () => {
    const base = {
      kind: "type",
      requestEpoch: 10,
      sourceEpoch: null,
      stage: "decoded",
      atUnixMs: 5,
    };
    expect(() => parseUiStampMessage(UI_LINE.replace('"type"', '"rename"'))).toThrow(
      /kind must be one of/,
    );
    expect(() => parseUiStampMessage(UI_LINE.replace('"decoded"', '"rendered"'))).toThrow(
      /stage must be one of/,
    );
    expect(() => parseUiStampValue({ ...base, requestEpoch: 0 })).toThrow(
      /requestEpoch must be a positive integer/,
    );
    expect(() => parseUiStampValue({ ...base, sourceEpoch: -3 })).toThrow(
      /sourceEpoch must be a positive integer/,
    );
    expect(() => parseUiStampValue({ ...base, atUnixMs: 1.5 })).toThrow(
      /atUnixMs must be a non-negative integer/,
    );
  });

  it("accepts a positive source epoch alongside a null one", () => {
    const stamp = parseUiStampValue({
      kind: "open",
      requestEpoch: 10,
      sourceEpoch: 7,
      stage: "input_dispatched",
      atUnixMs: 5,
    });
    expect(stamp.sourceEpoch).toBe(7);
  });
});

describe("capture scanning and the clock-domain anchor (WSP1L.2)", () => {
  it("scans captured stderr: noise is skipped, verter lines parse, malformed verter lines throw", () => {
    const scanned = collectStampMessages([
      "lapce: plugin loaded",
      ISSUED_LINE,
      "some plugin warning",
      UI_LINE,
    ]);
    expect(scanned.launchStamps).toHaveLength(1);
    expect(scanned.uiStamps).toHaveLength(1);
    expect(isStampMessage(ISSUED_LINE)).toBe(true);
    expect(isStampMessage("lapce: plugin loaded")).toBe(false);
    // A verter-prefixed line that does not parse fails the whole scan loud —
    // it is never silently dropped or guessed into the capture.
    expect(() => collectStampMessages([`${UI_STAMP_MESSAGE_PREFIX}{"kind":"type"`])).toThrow(
      /malformed verter ui-stamp JSON/,
    );
  });

  it("joins the stamp clock to the timeline clock only through a recorded anchor", () => {
    const anchor = {
      stampUnixMs: 1_758_000_000_000,
      timelineMs: 1_000,
      recordedAs: "capture-session clock anchor",
    };
    expect(stampUnixMsToTimelineMs(1_758_000_000_037, anchor)).toBe(1_037);
    expect(stampUnixMsToTimelineMs(anchor.stampUnixMs, anchor)).toBe(1_000);
    // No anchor, no conversion: an unanchored join is refused, never assumed.
    expect(() => stampUnixMsToTimelineMs(5, { ...anchor, stampUnixMs: 0.5 })).toThrow(
      /clock anchor/,
    );
  });
});
