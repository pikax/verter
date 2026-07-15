import { describe, it, expect } from "vitest";
import { join, dirname } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";
import { generatorFileUrl, GENERATOR_PATH } from "./corpus.js";

const __dirname = dirname(fileURLToPath(import.meta.url));
const GENERATOR_URL = pathToFileURL(
  join(
    __dirname,
    "..",
    "..",
    "..",
    "..",
    "test-corpora",
    "perf",
    "synthetic-15k",
    "generator",
    "generate.mjs",
  ),
).href;

interface GeneratorModule {
  buildCorpus: (config?: Record<string, number>) => {
    config: unknown;
    files: { relPath: string; bytes: Buffer }[];
  };
  hashCorpus: (files: { relPath: string; bytes: Buffer }[]) => string;
}

async function loadGenerator(): Promise<GeneratorModule> {
  return (await import(GENERATOR_URL)) as unknown as GeneratorModule;
}

// A small slice keeps the spec fast while still exercising every emitted shape
// (per-module type files, SFCs, the kernel/app split, all three tsconfigs).
const SLICE = { fileCount: 60, moduleCount: 10, importsPerFile: 4, compositeModuleCount: 3 };

describe("synthetic-15k corpus generator — OS-independent, deterministic", () => {
  it("emits POSIX-only paths and bytes (no backslash leaks the host separator)", async () => {
    const gen = await loadGenerator();
    const { files } = gen.buildCorpus(SLICE);
    expect(files.length).toBe(SLICE.fileCount + SLICE.moduleCount + 3); // SFCs + type files + 3 tsconfigs

    // The discriminating assertion: NO emitted relative path and NO emitted file
    // byte may contain a backslash. On Windows, an OS-dependent `path.join`
    // (relPath) or an OS-dependent path embedded in file CONTENT leaks `\` —
    // which differs from the Linux generation and breaks the cross-platform
    // content hash. The generator must normalize everything to forward slashes.
    for (const f of files) {
      expect(f.relPath.includes("\\"), `relPath leaked a backslash: ${f.relPath}`).toBe(false);
      const backslashByte = f.bytes.includes(0x5c);
      expect(backslashByte, `file content leaked a backslash byte: ${f.relPath}`).toBe(false);
    }
  });

  it("emits LF-only line endings in every text file (no CRLF leak)", async () => {
    const gen = await loadGenerator();
    const { files } = gen.buildCorpus(SLICE);
    for (const f of files) {
      // 0x0D (CR) must never appear — the generator normalizes to LF so a
      // CRLF checkout of the generator source cannot change the corpus bytes.
      expect(f.bytes.includes(0x0d), `file content leaked a CR byte: ${f.relPath}`).toBe(false);
    }
  });

  it("is byte-identical across two same-seed generations (deterministic hash)", async () => {
    const gen = await loadGenerator();
    const a = gen.buildCorpus({ ...SLICE, seed: 123 });
    const b = gen.buildCorpus({ ...SLICE, seed: 123 });
    expect(a.files.length).toBe(b.files.length);
    for (let i = 0; i < a.files.length; i++) {
      expect(a.files[i].relPath).toBe(b.files[i].relPath);
      expect(a.files[i].bytes.equals(b.files[i].bytes)).toBe(true);
    }
    expect(gen.hashCorpus(a.files)).toBe(gen.hashCorpus(b.files));
  });

  it("the content hash is independent of file emission order", async () => {
    const gen = await loadGenerator();
    const { files } = gen.buildCorpus(SLICE);
    const shuffled = [...files].reverse();
    expect(gen.hashCorpus(shuffled)).toBe(gen.hashCorpus(files));
  });
});

describe("generator file URL is Windows-portable", () => {
  it("round-trips through fileURLToPath (a raw file://<drive>/ URL does not on Windows)", () => {
    const url = generatorFileUrl();
    // A well-formed absolute file URL has the empty-host triple slash, so the
    // drive letter is never parsed as a URL host.
    expect(url.startsWith("file:///")).toBe(true);
    // The discriminating round-trip: fileURLToPath(url) must equal the original
    // absolute path. The old `file://${path}` form yields `file://D:/...` on
    // Windows (drive letter as host) and does NOT round-trip.
    expect(fileURLToPath(url)).toBe(GENERATOR_PATH);
  });
});

describe("synthetic-15k corpus generator — well-formed for ANY config", () => {
  it("a degenerate config that satisfies no import target emits no undeclared identifier", async () => {
    const gen = await loadGenerator();
    // importsPerFile 0 ⇒ no SFC gets an import target, so `ref0` (the first
    // imported-type usage) is NEVER declared. The SFC body must not reference it:
    // `renderSfc` must NOT always emit `return props.id + ref0.id;` — that would
    // reference an undeclared `ref0` in every SFC of such a degenerate corpus.
    const { files } = gen.buildCorpus({
      fileCount: 6,
      moduleCount: 3,
      importsPerFile: 0,
      compositeModuleCount: 1,
    });
    const vues = files.filter((f) => f.relPath.endsWith(".vue"));
    expect(vues.length).toBeGreaterThan(0);
    for (const f of vues) {
      const src = f.bytes.toString("utf-8");
      // With no import target, `ref0` must not appear at all (declared OR used).
      expect(src.includes("ref0"), `undeclared ref0 in ${f.relPath}`).toBe(false);
      // The SFC is still well-formed: the macros are present and the return is valid.
      expect(src).toContain("defineProps");
      expect(src).toContain("return props.id");
    }
  });

  it("a satisfiable-imports config still declares AND uses ref0 (control)", async () => {
    const gen = await loadGenerator();
    const { files } = gen.buildCorpus(SLICE); // importsPerFile 4 ⇒ every SFC has targets
    const vues = files.filter((f) => f.relPath.endsWith(".vue"));
    const withRef = vues.filter((f) => f.bytes.toString("utf-8").includes("const ref0"));
    // At least one SFC imports targets, so it declares `const ref0` AND uses it in
    // the return — the fix only drops the term when there is NO import target.
    expect(withRef.length).toBeGreaterThan(0);
    for (const f of withRef) {
      expect(f.bytes.toString("utf-8")).toContain("props.id + ref0.id");
    }
  });
});
