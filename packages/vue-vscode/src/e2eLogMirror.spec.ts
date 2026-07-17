import { describe, expect, it, vi } from "vitest";

import { installE2eLogMirror, type MirrorableLogChannel } from "./e2eLogMirror";

function channel(): MirrorableLogChannel {
  return {
    append: vi.fn(),
    appendLine: vi.fn(),
    info: vi.fn(),
    warn: vi.fn(),
    error: vi.fn(),
    debug: vi.fn(),
    trace: vi.fn(),
  };
}

describe("E2E output-channel log mirroring", () => {
  it("captures language-client append output as well as extension log methods", () => {
    const target = channel();
    const writes: string[] = [];
    installE2eLogMirror(target, (text) => writes.push(text));

    target.append("server partial");
    target.appendLine(" server line");
    target.info("provider ready", 3);

    expect(writes).toEqual(["server partial", " server line\n", "[INFO] provider ready 3\n"]);
  });

  it("preserves every original channel call", () => {
    const target = channel();
    const originalAppend = target.append as ReturnType<typeof vi.fn>;
    const originalAppendLine = target.appendLine as ReturnType<typeof vi.fn>;
    const originalWarn = target.warn as ReturnType<typeof vi.fn>;
    installE2eLogMirror(target, () => undefined);

    target.append("a");
    target.appendLine("b");
    target.warn("c", "d");

    expect(originalAppend).toHaveBeenCalledWith("a");
    expect(originalAppendLine).toHaveBeenCalledWith("b");
    expect(originalWarn).toHaveBeenCalledWith("c", "d");
  });
});
