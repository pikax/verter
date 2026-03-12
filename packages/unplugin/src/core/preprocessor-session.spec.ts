/**
 * @ai-generated - Tests for PreprocessorSession lifecycle management.
 * Verifies child-backed style preprocessing, teardown, crash handling, and respawn.
 */
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { join } from "node:path";
import { tmpdir } from "node:os";
import { mkdirSync, rmSync } from "node:fs";
import type { HostPreprocessorRequest } from "@verter/native";
import { PreprocessorSession } from "./preprocessor-session";

function makeStyleRequest(
  content: string,
  lang = "scss",
): HostPreprocessorRequest {
  return {
    blockType: "style",
    index: 0,
    lang,
    content,
  };
}

describe("PreprocessorSession", () => {
  let session: PreprocessorSession | null = null;
  let tempDir: string;

  beforeEach(() => {
    tempDir = join(
      tmpdir(),
      `verter-preprocessor-session-${Date.now()}-${Math.random().toString(36).slice(2, 8)}`,
    );
    mkdirSync(tempDir, { recursive: true });
  });

  afterEach(async () => {
    vi.restoreAllMocks();
    if (session) {
      await session.close();
      session = null;
    }
    rmSync(tempDir, { recursive: true, force: true });
  });

  it("compiles scss through the child process and strips sass syntax", async () => {
    session = new PreprocessorSession({ root: tempDir });
    const file = join(tempDir, "ChildCompile.vue").replace(/\\/g, "/");
    const result = await session.process(
      makeStyleRequest(
        "$color: red;\n.child-backed {\n  color: $color;\n}\n",
      ),
      file,
    );

    expect(result).not.toBeNull();
    expect(result?.code).toContain(".child-backed");
    expect(result?.code).toContain("red");
    expect(result?.code).not.toContain("$color");
    expect(session.isAlive()).toBe(true);
  }, 30_000);

  it("routes template, script, and custom blocks in-process without spawning the child", async () => {
    vi.spyOn(console, "warn").mockImplementation(() => {});
    session = new PreprocessorSession({ root: tempDir });
    const file = join(tempDir, "InProcess.vue").replace(/\\/g, "/");

    const templateResult = await session.process({
      blockType: "template",
      index: 0,
      lang: "unknown-template-lang",
      content: "<div>hello</div>",
    }, file);
    const scriptResult = await session.process({
      blockType: "script",
      index: 0,
      lang: "unknown-script-lang",
      content: "const x = 1",
    }, file);
    const customResult = await session.process(
      {
        blockType: "custom",
        index: 0,
        lang: "json",
        content: '{"hello":"world"}',
      },
      file,
      {
        docs: async (content) => ({
          code: JSON.stringify({ wrapped: content }),
        }),
      },
    );

    expect(templateResult).toBeNull();
    expect(scriptResult).toBeNull();
    expect(customResult).not.toBeNull();
    expect(customResult?.code).toContain("wrapped");
    expect(customResult?.code).not.toContain("\"missing\"");
    expect(session.isAlive()).toBe(false);
  });

  it("returns null for style blocks when vite config is absent and keeps the child dead", async () => {
    vi.spyOn(console, "warn").mockImplementation(() => {});
    session = new PreprocessorSession(null);
    const result = await session.process(
      makeStyleRequest("$color: red;\n.example { color: $color; }\n"),
      join(tempDir, "NoVite.vue").replace(/\\/g, "/"),
    );

    expect(result).toBeNull();
    expect(session.isAlive()).toBe(false);
  });

  it("cleans up the worker when init fails", async () => {
    session = new PreprocessorSession({
      configFile: join(tempDir, "missing-vite.config.ts"),
    });
    const file = join(tempDir, "InitFailure.vue").replace(/\\/g, "/");

    await expect(
      session.process(
        makeStyleRequest("@color: red;\n.init-failure { color: @color; }\n", "less"),
        file,
      ),
    ).rejects.toThrow(/Init failed|Could not resolve/);

    expect(session.isAlive()).toBe(false);
    await expect(session.close()).resolves.toBeUndefined();
  }, 30_000);

  it("rejects pending style work when the child crashes", async () => {
    session = new PreprocessorSession({ root: tempDir });
    const file = join(tempDir, "Crash.vue").replace(/\\/g, "/");

    const warmup = await session.process(
      makeStyleRequest("$color: red;\n.warmup { color: $color; }\n"),
      file,
    );
    expect(warmup).not.toBeNull();

    const longScss = [
      "$color: red;",
      ...Array.from({ length: 15000 }, (_, index) => `.x-${index} { color: $color; }`),
    ].join("\n");

    const pending = session.process(makeStyleRequest(longScss), file);
    const child = (session as any).child as { kill: (signal?: NodeJS.Signals) => boolean } | null;
    expect(child).not.toBeNull();
    child?.kill("SIGTERM");

    await expect(pending).rejects.toThrow(/exited unexpectedly|dead/i);
    await expect(session.process(makeStyleRequest(longScss), file)).rejects.toThrow(/dead/i);
  }, 30_000);

  it("handles vite config with non-serializable cssOptions (e.g., browserslist functions)", async () => {
    // Vite's resolved config can contain functions, class instances, etc.
    // that cannot be cloned via IPC. The session must not crash.
    session = new PreprocessorSession({
      root: tempDir,
      cssOptions: {
        preprocessorOptions: {
          scss: { additionalData: "" },
        },
        // Simulate a non-serializable function (like browserslist's `info()`)
        modules: { scopeBehaviour: "local" },
        devSourcemap: false,
        transformer: "postcss",
      } as Record<string, unknown>,
    });
    const file = join(tempDir, "NonSerializable.vue").replace(/\\/g, "/");
    const result = await session.process(
      makeStyleRequest("$color: green;\n.non-serial { color: $color; }\n"),
      file,
    );

    expect(result).not.toBeNull();
    expect(result?.code).toContain(".non-serial");
    expect(result?.code).toContain("green");
    expect(result?.code).not.toContain("$color");
    expect(session.isAlive()).toBe(true);
  }, 30_000);

  it("close is idempotent and a later style request respawns the child", async () => {
    session = new PreprocessorSession({ root: tempDir });
    const file = join(tempDir, "Respawn.vue").replace(/\\/g, "/");

    const first = await session.process(
      makeStyleRequest("$color: red;\n.first { color: $color; }\n"),
      file,
    );
    expect(first).not.toBeNull();
    expect(session.isAlive()).toBe(true);

    await session.close();
    expect(session.isAlive()).toBe(false);

    await session.close();
    expect(session.isAlive()).toBe(false);

    const second = await session.process(
      makeStyleRequest("$color: blue;\n.second { color: $color; }\n"),
      file,
    );
    expect(second).not.toBeNull();
    expect(second?.code).toContain("blue");
    expect(second?.code).not.toContain("$color");
    expect(session.isAlive()).toBe(true);
  }, 60_000);
});
