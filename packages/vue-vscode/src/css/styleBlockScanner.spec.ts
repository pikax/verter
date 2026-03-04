/**
 * Tests for style block scanner and position mapping.
 * Verifies that SCSS/LESS/CSS blocks are correctly detected and positioned.
 */
import { describe, it, expect } from "vitest";
import { scanStyleBlocks, findStyleBlockAt } from "./styleBlockScanner";

describe("scanStyleBlocks", () => {
  it("detects plain CSS style blocks", () => {
    const source = `<template><div>hello</div></template>
<style>
.foo { color: red; }
</style>`;
    const blocks = scanStyleBlocks(source);
    expect(blocks).toHaveLength(1);
    expect(blocks[0].lang).toBe("css");
    expect(blocks[0].index).toBe(0);
  });

  it("detects SCSS style blocks", () => {
    const source = `<template><div>hello</div></template>
<style lang="scss">
.foo {
  &__bar { color: red; }
}
</style>`;
    const blocks = scanStyleBlocks(source);
    expect(blocks).toHaveLength(1);
    expect(blocks[0].lang).toBe("scss");
    expect(blocks[0].index).toBe(0);
  });

  it("detects multiple style blocks with different langs", () => {
    const source = `<template><div></div></template>
<style>
.a { color: red; }
</style>
<style lang="scss">
.b { color: blue; }
</style>`;
    const blocks = scanStyleBlocks(source);
    expect(blocks).toHaveLength(2);
    expect(blocks[0].lang).toBe("css");
    expect(blocks[0].index).toBe(0);
    expect(blocks[1].lang).toBe("scss");
    expect(blocks[1].index).toBe(1);
  });

  it("contentStartOffset/EndOffset correctly bracket style content", () => {
    const source = `<template><div></div></template>
<style lang="scss">
.foo { color: red; }
</style>`;
    const blocks = scanStyleBlocks(source);
    expect(blocks).toHaveLength(1);

    const content = source.slice(blocks[0].contentStartOffset, blocks[0].contentEndOffset);
    expect(content).toContain(".foo { color: red; }");
    // Negative: content should NOT contain the style tag itself
    expect(content).not.toContain("<style");
    expect(content).not.toContain("</style");
  });
});

describe("findStyleBlockAt", () => {
  it("finds SCSS block when cursor is inside it", () => {
    const source = `<template><div></div></template>
<style lang="scss">
.foo { color: red; }
</style>`;
    const blocks = scanStyleBlocks(source);

    // Line 2 is inside the SCSS block
    const block = findStyleBlockAt(blocks, source, 2, 5);
    expect(block).toBeDefined();
    expect(block!.lang).toBe("scss");
  });

  it("returns undefined when cursor is outside style blocks", () => {
    const source = `<template><div></div></template>
<style lang="scss">
.foo { color: red; }
</style>`;
    const blocks = scanStyleBlocks(source);

    // Line 0 is in the template
    const block = findStyleBlockAt(blocks, source, 0, 5);
    expect(block).toBeUndefined();
  });
});
