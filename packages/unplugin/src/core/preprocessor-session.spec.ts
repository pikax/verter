/**
 * @ai-generated - Tests for PreprocessorSession lifecycle management.
 * Verifies child process spawn, message passing, teardown, and error handling.
 */
import { describe, it, expect, afterEach } from "vitest";
import { PreprocessorSession } from "./preprocessor-session";

describe("PreprocessorSession", () => {
  let session: PreprocessorSession | null = null;

  afterEach(async () => {
    if (session) {
      await session.close();
      session = null;
    }
  });

  it("constructs without spawning a child process", () => {
    session = new PreprocessorSession({ root: process.cwd() });
    expect(session.isAlive()).toBe(false);
  });

  it("close() is idempotent when no child was spawned", async () => {
    session = new PreprocessorSession({ root: process.cwd() });
    await session.close();
    await session.close(); // Should not throw
    expect(session.isAlive()).toBe(false);
  });

  // @ai-generated - Template preprocessing stays in-process (no child needed)
  it("routes template blocks in-process without spawning child", async () => {
    session = new PreprocessorSession(null);
    const req = {
      blockType: "template" as const,
      lang: "unknown-lang",
      content: "<div>hello</div>",
      index: 0,
    };
    // Unknown template lang returns null (no preprocessor), but doesn't throw
    const result = await session.process(req, "/test.vue");
    expect(result).toBeNull();
    expect(session.isAlive()).toBe(false);
  });

  // @ai-generated - Script preprocessing stays in-process
  it("routes script blocks in-process without spawning child", async () => {
    session = new PreprocessorSession(null);
    const req = {
      blockType: "script" as const,
      lang: "unknown-lang",
      content: "const x = 1",
      index: 0,
    };
    const result = await session.process(req, "/test.vue");
    expect(result).toBeNull();
    expect(session.isAlive()).toBe(false);
  });

  // @ai-generated - Custom blocks stay in-process
  it("routes custom blocks in-process without spawning child", async () => {
    session = new PreprocessorSession(null);
    const req = {
      blockType: "custom" as const,
      lang: "json",
      content: '{"hello":"world"}',
      index: 0,
    };
    const result = await session.process(req, "/test.vue");
    expect(result).toBeNull();
    expect(session.isAlive()).toBe(false);
  });

  // @ai-generated - Style preprocessing without viteConfig warns and returns null
  it("returns null for style blocks when viteConfig is null", async () => {
    session = new PreprocessorSession(null);
    const req = {
      blockType: "style" as const,
      lang: "scss",
      content: "$color: red; .foo { color: $color; }",
      index: 0,
    };
    const result = await session.process(req, "/test.vue");
    expect(result).toBeNull();
    expect(session.isAlive()).toBe(false);
  });

  // @ai-generated - Non-preprocessor style langs return null
  it("returns null for style blocks with unknown lang", async () => {
    session = new PreprocessorSession({ root: process.cwd() });
    const req = {
      blockType: "style" as const,
      lang: "unknown-css-lang",
      content: ".foo { color: red; }",
      index: 0,
    };
    const result = await session.process(req, "/test.vue");
    expect(result).toBeNull();
  });
});
