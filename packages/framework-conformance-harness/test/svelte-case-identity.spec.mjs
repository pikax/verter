// Identity-stability property for Svelte official case_ids: synthetic
// "old manifest + new checkout" fixture, no network/real-checkout
// dependency. Proves the durable-identity contract in
// svelte-case-identity-ledger.md directly against the generator, rather
// than only observing it as a byte-diff on the real 5.56.8 -> 5.56.10 bump.

import { execFileSync } from "node:child_process";
import { createHash } from "node:crypto";
import { mkdirSync, mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";

import { afterEach, beforeEach, describe, expect, it } from "vitest";

import {
  svelteManifest,
  SVELTE_CASE_ID_SALT,
} from "../../../docs/arch/refactor/rev11/evidence/framework-conformance/generate-official-case-manifests.mjs";

let root;

function git(...args) {
  return execFileSync("git", ["-C", root, ...args], { encoding: "utf8" }).trim();
}

function writeSample(suite, sample, content) {
  const dir = join(root, "packages", "svelte", "tests", suite, "samples", sample);
  mkdirSync(dir, { recursive: true });
  writeFileSync(join(dir, "input.svelte"), content);
}

function removeSample(suite, sample) {
  rmSync(join(root, "packages", "svelte", "tests", suite, "samples", sample), {
    recursive: true,
    force: true,
  });
}

// Writes a file directly under packages/svelte/tests/<suite>/ with NO
// samples/ subdirectory, so svelteManifest()'s sampleDirs detection for this
// suite comes back empty and the suite takes the suite-sentinel branch
// (declaration_kind: "suite-sentinel") rather than the sample-directory
// branch — the two branches hash against SVELTE_CASE_ID_SALT at two
// separate call sites in the generator.
function writeSuiteFile(suite, file, content) {
  const dir = join(root, "packages", "svelte", "tests", suite);
  mkdirSync(dir, { recursive: true });
  writeFileSync(join(dir, file), content);
}

beforeEach(() => {
  root = mkdtempSync(join(tmpdir(), "svelte-case-identity-"));
  git("init", "-q");
  git("config", "user.email", "test@example.com");
  git("config", "user.name", "test");
});

afterEach(() => {
  rmSync(root, { recursive: true, force: true });
});

describe("Svelte case_id durability across a synthetic version bump", () => {
  it("an unchanged locator keeps the same case_id even when its content changes; a removed locator's row disappears; a new locator mints a fresh id", () => {
    // "old" state: unchanged-case (v1 content) + removed-case.
    writeSample("demo-suite", "unchanged-case", "<div>v1</div>");
    writeSample("demo-suite", "removed-case", "<div>gone-next-version</div>");
    git("add", "-A");
    git("commit", "-q", "-m", "old");
    const oldRows = svelteManifest(root);

    // "new" state: unchanged-case gets a real content edit (still the same
    // logical test), removed-case is deleted, added-case is new.
    writeSample("demo-suite", "unchanged-case", "<div>v2 — content actually changed</div>");
    removeSample("demo-suite", "removed-case");
    writeSample("demo-suite", "added-case", "<div>brand new upstream case</div>");
    git("add", "-A");
    git("commit", "-q", "-m", "new");
    const newRows = svelteManifest(root);

    const byLocator = (rows) => new Map(rows.map((row) => [row.source_locator, row]));
    const oldByLoc = byLocator(oldRows);
    const newByLoc = byLocator(newRows);

    const unchangedLocator = "packages/svelte/tests/demo-suite/samples/unchanged-case/";
    const removedLocator = "packages/svelte/tests/demo-suite/samples/removed-case/";
    const addedLocator = "packages/svelte/tests/demo-suite/samples/added-case/";

    // Same locator, same case_id, even though content (source_object) moved.
    expect(oldByLoc.get(unchangedLocator).case_id).toBe(newByLoc.get(unchangedLocator).case_id);
    expect(oldByLoc.get(unchangedLocator).source_object).not.toBe(
      newByLoc.get(unchangedLocator).source_object,
    );

    // Removed locator's row is gone in the new state — no id is reused for it.
    expect(oldByLoc.has(removedLocator)).toBe(true);
    expect(newByLoc.has(removedLocator)).toBe(false);

    // Added locator mints a fresh id, present only in the new state.
    expect(oldByLoc.has(addedLocator)).toBe(false);
    expect(newByLoc.has(addedLocator)).toBe(true);

    // No id collision between the added case and any old id.
    const oldIds = new Set(oldRows.map((row) => row.case_id));
    expect(oldIds.has(newByLoc.get(addedLocator).case_id)).toBe(false);
  });

  it("case_id is independent of the frozen salt's origin string looking like a version — changing what the checkout HEAD resolves to does not change ids for the same locator", () => {
    writeSample("demo-suite", "stable-case", "<div>anything</div>");
    git("add", "-A");
    git("commit", "-q", "-m", "commit-a");
    const firstPassRows = svelteManifest(root);

    // A second commit that touches an unrelated file only — the pinned
    // upstream "version" (HEAD) moved, but this locator's content and path
    // did not.
    writeFileSync(join(root, "unrelated.txt"), "noise");
    git("add", "-A");
    git("commit", "-q", "-m", "commit-b");
    const secondPassRows = svelteManifest(root);

    const locator = "packages/svelte/tests/demo-suite/samples/stable-case/";
    const firstId = firstPassRows.find((row) => row.source_locator === locator).case_id;
    const secondId = secondPassRows.find((row) => row.source_locator === locator).case_id;
    expect(secondId).toBe(firstId);
  });

  it("SVELTE_CASE_ID_SALT is frozen at the historical value existing case_ids were minted from — regressing this constant is the one change that would break every existing evidence_id cross-reference", () => {
    expect(SVELTE_CASE_ID_SALT).toBe("svelte-5.56.8");
  });

  it("a generated case_id for a sample-directory row matches the documented formula computed independently against the frozen salt literal — catches the salt being silently swapped for the live version pin at the sample-directory hash call site, which the durability tests above cannot (they only ever observe SVELTE_CASE_ID_SALT and EXPECTED_SVELTE as two fixed constants within one process run, so swapping which one is wired in produces identical output within that run). This covers ONLY the sample-directory branch's call site — see the suite-sentinel test below for the other one.", () => {
    writeSample("demo-suite", "formula-check", "<div>anything</div>");
    git("add", "-A");
    git("commit", "-q", "-m", "formula-check");
    const rows = svelteManifest(root);

    const locator = "packages/svelte/tests/demo-suite/samples/formula-check/";
    const row = rows.find((row) => row.source_locator === locator);
    expect(row.declaration_kind).toBe("sample-directory");
    const actual = row.case_id;

    // Recomputed against the "svelte-5.56.8" literal from
    // svelte-case-identity-ledger.md's identity contract directly — NOT via
    // the imported SVELTE_CASE_ID_SALT constant, so this stays a real
    // external oracle even if the generator's wiring regresses. If the
    // sample-directory hash call site in svelteManifest() starts hashing
    // against EXPECTED_SVELTE (the live upstream commit pin, which is a real
    // commit in this synthetic repo and therefore not "svelte-5.56.8")
    // instead of the frozen salt, `actual` diverges from `expected` and this
    // fails.
    const expected =
      "SVELTE-" +
      createHash("sha256")
        .update(`svelte-5.56.8\0${locator}`)
        .digest("hex")
        .slice(0, 20)
        .toUpperCase();

    expect(actual).toBe(expected);
  });

  it("a generated case_id for a suite-sentinel row (a suite with no samples/ subdirectory) matches the documented formula computed independently against the frozen salt literal — catches the salt being silently swapped for the live version pin at the suite-sentinel hash call site, the one call site the sample-directory formula test above cannot reach", () => {
    writeSuiteFile("sentinel-suite", "index.js", "// no samples/ subdirectory for this suite");
    git("add", "-A");
    git("commit", "-q", "-m", "sentinel-formula-check");
    const rows = svelteManifest(root);

    const locator = "packages/svelte/tests/sentinel-suite/";
    const row = rows.find((row) => row.source_locator === locator);
    expect(row.declaration_kind).toBe("suite-sentinel");
    const actual = row.case_id;

    // Same independent oracle as the sample-directory test above, computed
    // against the suite-sentinel locator (no trailing samples/<name>/
    // segment). If the suite-sentinel hash call site in svelteManifest()
    // starts hashing against EXPECTED_SVELTE instead of the frozen salt,
    // `actual` diverges from `expected` and this fails — independently of
    // whatever the sample-directory call site does.
    const expected =
      "SVELTE-" +
      createHash("sha256")
        .update(`svelte-5.56.8\0${locator}`)
        .digest("hex")
        .slice(0, 20)
        .toUpperCase();

    expect(actual).toBe(expected);
  });
});
