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

describe("langAttributeRange", () => {
  // @ai-generated - Tests langAttributeRange for various attribute formats
  it("is absent for plain <style> without lang attribute", () => {
    const source = `<style>
.foo { color: red; }
</style>`;
    const blocks = scanStyleBlocks(source);
    expect(blocks).toHaveLength(1);
    expect(blocks[0].langAttributeRange).toBeUndefined();
  });

  it('covers the full lang="scss" attribute text with double quotes', () => {
    const source = `<style lang="scss">
.foo { color: red; }
</style>`;
    const blocks = scanStyleBlocks(source);
    expect(blocks).toHaveLength(1);
    const range = blocks[0].langAttributeRange;
    expect(range).toBeDefined();
    // <style lang="scss"> is on line 0
    expect(range!.startLine).toBe(0);
    expect(range!.endLine).toBe(0);
    // Extract the text covered by the range
    const line = source.split("\n")[0];
    const text = line.slice(range!.startCol, range!.endCol);
    expect(text).toBe('lang="scss"');
  });

  it("covers the full lang='sass' attribute text with single quotes", () => {
    const source = `<style lang='sass'>
.foo
  color: red
</style>`;
    const blocks = scanStyleBlocks(source);
    expect(blocks).toHaveLength(1);
    const range = blocks[0].langAttributeRange;
    expect(range).toBeDefined();
    const line = source.split("\n")[0];
    const text = line.slice(range!.startCol, range!.endCol);
    expect(text).toBe("lang='sass'");
  });

  it("covers lang=stylus attribute text without quotes", () => {
    const source = `<style lang=stylus>
.foo
  color red
</style>`;
    const blocks = scanStyleBlocks(source);
    expect(blocks).toHaveLength(1);
    const range = blocks[0].langAttributeRange;
    expect(range).toBeDefined();
    const line = source.split("\n")[0];
    const text = line.slice(range!.startCol, range!.endCol);
    expect(text).toBe("lang=stylus");
  });

  it("works alongside scoped attribute", () => {
    const source = `<style scoped lang="less">
.foo { color: red; }
</style>`;
    const blocks = scanStyleBlocks(source);
    expect(blocks).toHaveLength(1);
    expect(blocks[0].scoped).toBe(true);
    expect(blocks[0].lang).toBe("less");
    const range = blocks[0].langAttributeRange;
    expect(range).toBeDefined();
    const line = source.split("\n")[0];
    const text = line.slice(range!.startCol, range!.endCol);
    expect(text).toBe('lang="less"');
  });

  it("tracks langAttributeRange when the opening tag spans multiple lines", () => {
    const source = `<style
  scoped
  lang="sass">
.foo
  color: red
</style>`;
    const blocks = scanStyleBlocks(source);
    expect(blocks).toHaveLength(1);
    const range = blocks[0].langAttributeRange;
    expect(range).toBeDefined();
    expect(range!.startLine).toBe(2);
    expect(range!.endLine).toBe(2);
    const line = source.split("\n")[2];
    const text = line.slice(range!.startCol, range!.endCol);
    expect(text).toBe('lang="sass"');
  });

  it("handles lang attribute after template on next line", () => {
    const source = `<template><div>hello</div></template>
<style lang="sass">
.foo
  color: red
</style>`;
    const blocks = scanStyleBlocks(source);
    expect(blocks).toHaveLength(1);
    const range = blocks[0].langAttributeRange;
    expect(range).toBeDefined();
    // The style tag is on line 1
    expect(range!.startLine).toBe(1);
    expect(range!.endLine).toBe(1);
    const line = source.split("\n")[1];
    const text = line.slice(range!.startCol, range!.endCol);
    expect(text).toBe('lang="sass"');
  });

  it("second style block has correct langAttributeRange", () => {
    const source = `<style>
.a { color: red; }
</style>
<style lang="scss" scoped>
.b { color: blue; }
</style>`;
    const blocks = scanStyleBlocks(source);
    expect(blocks).toHaveLength(2);
    // First block: no lang attribute
    expect(blocks[0].langAttributeRange).toBeUndefined();
    // Second block: has lang attribute
    const range = blocks[1].langAttributeRange;
    expect(range).toBeDefined();
    expect(range!.startLine).toBe(3);
    const line = source.split("\n")[3];
    const text = line.slice(range!.startCol, range!.endCol);
    expect(text).toBe('lang="scss"');
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
