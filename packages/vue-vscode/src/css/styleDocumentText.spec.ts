import { describe, it, expect } from "vitest";
import { directStyleDocumentText } from "./styleDocumentText";

/**
 * Regression guard for the color-decorator mis-map: the CSS service's
 * position mapping (`toCssPosition`/`toSfcPosition`) is pure line arithmetic
 * anchored at the block content start, so the parsed CSS document text MUST
 * be the verbatim authored slice. Feeding the compiled style output (which
 * trims the leading newline and rewrites scoped selectors) shifts every
 * mapped range one line up — the observed defect rendered a color chip ON
 * the FIRST class name of `<style>` instead of on the color value.
 */
describe("directStyleDocumentText", () => {
  const sfc =
    '<template><div class="first"></div></template>\n' +
    "<style scoped>\n.first {\n  color: #abc;\n}\n</style>\n";
  const contentStartOffset = sfc.indexOf(">", sfc.indexOf("<style")) + 1;
  const contentEndOffset = sfc.indexOf("</style>", contentStartOffset);
  const block = { contentStartOffset, contentEndOffset, contentStartLine: 1 };

  // Mirrors CssService.toSfcPosition (line arithmetic, no source maps).
  function toSfcLine(block: { contentStartLine: number }, cssLine: number): number {
    return cssLine + block.contentStartLine;
  }

  it("returns the verbatim authored slice including the leading newline", () => {
    const text = directStyleDocumentText(sfc, block);
    expect(text).toBe("\n.first {\n  color: #abc;\n}\n");
    expect(text.startsWith("\n")).toBe(true);
  });

  it("keeps the color value on its authored line under the line-arithmetic mapping", () => {
    const text = directStyleDocumentText(sfc, block);

    // The color sits on css-doc line 2 ("  color: #abc;").
    const cssLines = text.split("\n");
    const colorCssLine = cssLines.findIndex((l) => l.includes("#abc"));
    expect(colorCssLine).toBe(2);

    // Mapped back to the SFC, that must be the authored "  color: #abc;"
    // line — NOT the ".first {" line above it.
    const sfcLines = sfc.split("\n");
    const mapped = toSfcLine(block, colorCssLine);
    expect(sfcLines[mapped]).toContain("#abc");
    expect(sfcLines[mapped]).not.toContain(".first");
  });

  it("DISCRIMINATES: a compiled-style-shaped text (leading trim) would mis-map onto the class name", () => {
    // The compiled style virtual output starts at the first rule (no leading
    // newline) — the exact shape that produced the defect.
    const compiledShaped = directStyleDocumentText(sfc, block).replace(/^\n/, "");

    const cssLines = compiledShaped.split("\n");
    const colorCssLine = cssLines.findIndex((l) => l.includes("#abc"));

    const sfcLines = sfc.split("\n");
    const misMapped = toSfcLine(block, colorCssLine);
    // Under the same arithmetic the trimmed text lands the chip ON the class
    // name — proving the verbatim slice is load-bearing.
    expect(sfcLines[misMapped]).toContain(".first");
    expect(sfcLines[misMapped]).not.toContain("#abc");
  });
});
