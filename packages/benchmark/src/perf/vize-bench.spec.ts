import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { describe, expect, it } from "vitest";

/**
 * The offline vize comparison is a plain-text log consumer (manager-run, piped to
 * the CTO). Its warning lines must be ASCII so a non-UTF-8 log pipe cannot mangle
 * them. The AXIS-B caveat must NOT carry the warning-sign emoji (U+26A0 plus the
 * U+FE0F variation selector); it uses an ASCII `WARNING:` marker instead.
 */
describe("vize-bench warning output is ASCII", () => {
  const source = readFileSync(fileURLToPath(new URL("./vize-bench.ts", import.meta.url)), "utf-8");
  const WARNING_SIGN = String.fromCodePoint(0x26a0); // the warning-sign emoji base
  const VARIATION_SELECTOR = String.fromCodePoint(0xfe0f); // the emoji variation selector

  it("the AXIS-B caveat line carries no warning-sign emoji", () => {
    expect(source.includes(WARNING_SIGN)).toBe(false);
    expect(source.includes(VARIATION_SELECTOR)).toBe(false);
  });

  it("the AXIS-B caveat line uses an ASCII WARNING: marker", () => {
    expect(source).toMatch(/WARNING:\s*\$\{b\.caveat\}/);
  });
});
