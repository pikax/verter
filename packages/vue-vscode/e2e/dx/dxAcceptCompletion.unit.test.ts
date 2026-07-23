import { describe, expect, it } from "vitest";

import {
  ACCEPT_SUGGESTION_COMMAND,
  acceptCompletion,
  classifyAcceptOutcome,
  extractStaticImportText,
  TRIGGER_SUGGEST_COMMAND,
  type AcceptCompletionDeps,
} from "./dxAcceptCompletion";

describe("classifyAcceptOutcome", () => {
  it("counts the accept as real only when BOTH document and import text changed", () => {
    expect(
      classifyAcceptOutcome({
        docBefore: "a",
        docAfter: "aMyComp",
        importBefore: "",
        importAfter: "import { MyComp } from './c'",
      }),
    ).toEqual({ docChanged: true, importChanged: true, accepted: true });
  });

  it("is not accepted when only the document changed (no auto-import edit)", () => {
    expect(
      classifyAcceptOutcome({
        docBefore: "a",
        docAfter: "aMyComp",
        importBefore: "x",
        importAfter: "x",
      }).accepted,
    ).toBe(false);
  });

  it("is not accepted when only the import changed (no document mutation)", () => {
    expect(
      classifyAcceptOutcome({
        docBefore: "a",
        docAfter: "a",
        importBefore: "",
        importAfter: "import { MyComp } from './c'",
      }).accepted,
    ).toBe(false);
  });
});

describe("extractStaticImportText", () => {
  it("does not mistake an accepted identifier edit for an auto-import edit", () => {
    const before = `<script setup lang="ts">
import { ref } from "vue";
const doubled = comput(() => ref(1).value * 2);
</script>`;
    const afterWithoutImportEdit = before.replace("comput(", "computed(");

    expect(extractStaticImportText(afterWithoutImportEdit)).toBe(extractStaticImportText(before));
    expect(extractStaticImportText(before)).toBe('import { ref } from "vue";');
  });

  it("captures multiline and type-only static imports", () => {
    const script = `<script lang="ts">
import type {
  Component,
} from "vue";
import "./side-effect";
const value = import("./dynamic");
</script>`;

    expect(extractStaticImportText(script)).toContain("import type");
    expect(extractStaticImportText(script)).toContain('import "./side-effect"');
    expect(extractStaticImportText(script)).not.toContain("dynamic");
  });
});

/** A fake editor where accepting a suggestion mutates both doc and import text. */
function fakeWorld(opts: { acceptMutatesDoc?: boolean; acceptMutatesImport?: boolean } = {}) {
  const state = { doc: "<MyComp", imports: "" };
  const commands: string[] = [];
  const deps: AcceptCompletionDeps = {
    runCommand(command: string) {
      commands.push(command);
      if (command === ACCEPT_SUGGESTION_COMMAND) {
        if (opts.acceptMutatesDoc ?? true) state.doc = "<MyComp />";
        if (opts.acceptMutatesImport ?? true) state.imports = "import MyComp from './MyComp.vue'";
      }
      return Promise.resolve();
    },
    readDocText: () => state.doc,
    readImportText: () => state.imports,
  };
  return { deps, commands, state };
}

describe("acceptCompletion", () => {
  it("invokes triggerSuggest THEN acceptSelectedSuggestion, in that order", async () => {
    const { deps, commands } = fakeWorld();
    await acceptCompletion(deps);
    expect(commands).toEqual([TRIGGER_SUGGEST_COMMAND, ACCEPT_SUGGESTION_COMMAND]);
  });

  it("returns an accepted outcome with the before/after snapshots when both changed", async () => {
    const { deps } = fakeWorld();
    const outcome = await acceptCompletion(deps);
    expect(outcome.accepted).toBe(true);
    expect(outcome.docChanged).toBe(true);
    expect(outcome.importChanged).toBe(true);
    expect(outcome.docBefore).toBe("<MyComp");
    expect(outcome.docAfter).toBe("<MyComp />");
    expect(outcome.importAfter).toContain("import MyComp");
  });

  it("throws when the accept path did not add an import (the gate's failure mode)", async () => {
    const { deps, commands } = fakeWorld({ acceptMutatesImport: false });
    await expect(acceptCompletion(deps)).rejects.toThrow(/import/i);
    // Both commands must still have been attempted before the verdict.
    expect(commands).toEqual([TRIGGER_SUGGEST_COMMAND, ACCEPT_SUGGESTION_COMMAND]);
  });

  it("throws when the accept path did not mutate the document", async () => {
    const { deps } = fakeWorld({ acceptMutatesDoc: false });
    await expect(acceptCompletion(deps)).rejects.toThrow(/document|doc/i);
  });

  it("awaits the optional settle hook after each command", async () => {
    const { deps, commands } = fakeWorld();
    const settleAt: number[] = [];
    await acceptCompletion({
      ...deps,
      settle: () => {
        settleAt.push(commands.length);
        return Promise.resolve();
      },
    });
    // settle runs once after triggerSuggest (1 cmd) and once after accept (2 cmds).
    expect(settleAt).toEqual([1, 2]);
  });
});
