import { describe, expect, it } from "vitest";
import type * as vscode from "vscode";

import {
  ACCEPT_SUGGESTION_COMMAND,
  acceptCompletion,
  classifyAcceptOutcome,
  extractStaticImportText,
  TRIGGER_SUGGEST_COMMAND,
  vscodeAcceptCompletionDeps,
  type AcceptCompletionDeps,
  type AcceptOutcome,
  type AcceptSubscription,
  type VsCodeAcceptApi,
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

const SCRIPT_BEFORE = `<script setup lang="ts">
import { ref } from "vue";
const doubled = comput(() => ref(1).value * 2);
</script>`;
const SCRIPT_IDENTIFIER_ACCEPTED = SCRIPT_BEFORE.replace("comput(", "computed(");
const SCRIPT_IMPORT_APPLIED = SCRIPT_IDENTIFIER_ACCEPTED.replace(
  'import { ref } from "vue";',
  'import { computed, ref } from "vue";',
);

/** How the simulated editor answers an accept. */
interface AcceptBehaviour {
  /** The accept inserts the focused item's identifier. */
  readonly insertsIdentifier: boolean;
  /** When the import edit lands: with the insert, after `resolveArrives()`, or never. */
  readonly importEdit: "with-insert" | "on-resolve" | "never";
}

/**
 * A simulated editor with VS Code's timing: the suggestion widget populates only
 * when the test calls `listArrives()`, an accept issued while it is not showing a
 * focused item does nothing, and an import carried by `completionItem/resolve`
 * lands only when the test calls `resolveArrives()`. Every transition is driven by
 * the test, never by elapsed time.
 */
class SimulatedEditor {
  text = SCRIPT_BEFORE;
  readonly log: string[] = [];
  private focused: string | undefined;
  private importPending = false;
  private readonly focusListeners = new Set<(insertText: string) => void>();
  private readonly changeListeners = new Set<() => void>();
  private deadline: (() => void) | undefined;

  constructor(private readonly behaviour: AcceptBehaviour) {}

  get liveSubscriptions(): number {
    return this.focusListeners.size + this.changeListeners.size + (this.deadline ? 1 : 0);
  }

  get deadlineArmed(): boolean {
    return this.deadline !== undefined;
  }

  /** The suggestion widget's list arrives and it focuses `insertText`. */
  listArrives(insertText = "computed"): void {
    this.focused = insertText;
    this.log.push(`focus:${insertText}`);
    for (const listener of [...this.focusListeners]) listener(insertText);
  }

  /** The pending `completionItem/resolve` answers and its import edit is applied. */
  resolveArrives(): void {
    if (!this.importPending) throw new Error("no resolve is pending");
    this.importPending = false;
    this.edit(SCRIPT_IMPORT_APPLIED, "import-edit");
  }

  /** The hang guard fires. */
  expireDeadline(): void {
    const onExpired = this.deadline;
    if (!onExpired) throw new Error("no deadline is armed");
    onExpired();
  }

  run(command: string): void {
    this.log.push(command);
    if (command === TRIGGER_SUGGEST_COMMAND) return;
    if (command !== ACCEPT_SUGGESTION_COMMAND) throw new Error(`unexpected command ${command}`);
    // `acceptSelectedSuggestion` requires a visible widget with a focused item.
    if (this.focused === undefined) return;
    this.focused = undefined;
    if (!this.behaviour.insertsIdentifier) return;
    if (this.behaviour.importEdit === "with-insert") {
      this.edit(SCRIPT_IMPORT_APPLIED, "insert+import");
    } else {
      this.edit(SCRIPT_IDENTIFIER_ACCEPTED, "insert");
      this.importPending = this.behaviour.importEdit === "on-resolve";
    }
  }

  private edit(text: string, label: string): void {
    this.text = text;
    this.log.push(`edit:${label}`);
    for (const listener of [...this.changeListeners]) listener();
  }

  deps(overrides: Partial<AcceptCompletionDeps> = {}): AcceptCompletionDeps {
    return {
      expectedSuggestion: "computed",
      runCommand: (command) => {
        this.run(command);
        return Promise.resolve();
      },
      readDocText: () => this.text,
      readImportText: () => extractStaticImportText(this.text),
      observeFocusedSuggestion: (listener) => this.subscribe(this.focusListeners, listener),
      observeDocumentChange: (listener) => this.subscribe(this.changeListeners, listener),
      armDeadline: (onExpired) => {
        if (this.deadline) throw new Error("a deadline is already armed");
        this.deadline = onExpired;
        return {
          dispose: () => {
            this.deadline = undefined;
          },
        };
      },
      ...overrides,
    };
  }

  private subscribe<T>(set: Set<T>, listener: T): AcceptSubscription {
    set.add(listener);
    return { dispose: () => set.delete(listener) };
  }
}

/** Let every pending promise continuation run (one macrotask turn; no clock). */
const drain = (): Promise<void> => new Promise((resolve) => setImmediate(resolve));

/**
 * Track a driver run without awaiting it, so the test can step the editor and then
 * read the settled state after a {@link drain} (a run that never settles is then a
 * failed assertion, not a hung test).
 */
function start(run: Promise<AcceptOutcome>) {
  const state: { outcome?: AcceptOutcome; error?: unknown } = {};
  void run.then(
    (outcome) => {
      state.outcome = outcome;
    },
    (error: unknown) => {
      state.error = error;
    },
  );
  return { state };
}

describe("acceptCompletion", () => {
  it("waits for the widget to focus the expected item before its one accept (delayed list)", async () => {
    const editor = new SimulatedEditor({ insertsIdentifier: true, importEdit: "with-insert" });
    const run = start(acceptCompletion(editor.deps()));

    await drain();
    // The list has not arrived: an accept now would be a silent no-op.
    expect(editor.log).toEqual([TRIGGER_SUGGEST_COMMAND]);
    expect(run.state).toEqual({});

    editor.listArrives();
    await drain();

    expect(run.state.error).toBeUndefined();
    expect(editor.log).toEqual([
      TRIGGER_SUGGEST_COMMAND,
      "focus:computed",
      ACCEPT_SUGGESTION_COMMAND,
      "edit:insert+import",
    ]);
    expect(run.state.outcome).toMatchObject({
      accepted: true,
      docChanged: true,
      importChanged: true,
      commands: [TRIGGER_SUGGEST_COMMAND, ACCEPT_SUGGESTION_COMMAND],
    });
  });

  it("waits for an import edit that lands after the identifier (delayed resolve)", async () => {
    const editor = new SimulatedEditor({ insertsIdentifier: true, importEdit: "on-resolve" });
    editor.listArrives();
    const run = start(acceptCompletion(editor.deps()));

    // The focus already happened before the trigger; only a focus AFTER it counts.
    await drain();
    expect(editor.log).toEqual(["focus:computed", TRIGGER_SUGGEST_COMMAND]);
    editor.listArrives();
    await drain();
    // The identifier is in, the import is not yet: the verdict must still be open.
    expect(editor.log[editor.log.length - 1]).toBe("edit:insert");
    expect(run.state).toEqual({});

    editor.resolveArrives();
    await drain();

    expect(run.state.error).toBeUndefined();
    expect(run.state.outcome?.importAfter).toContain("computed");
    expect(run.state.outcome?.commands).toEqual([
      TRIGGER_SUGGEST_COMMAND,
      ACCEPT_SUGGESTION_COMMAND,
    ]);
  });

  it("does not accept a different focused item, and accepts once the expected one is focused", async () => {
    const editor = new SimulatedEditor({ insertsIdentifier: true, importEdit: "with-insert" });
    const run = start(acceptCompletion(editor.deps()));
    await drain();

    editor.listArrives("computedAsync");
    await drain();
    expect(editor.log).not.toContain(ACCEPT_SUGGESTION_COMMAND);

    editor.listArrives("computed");
    await drain();
    expect(run.state.error).toBeUndefined();
    expect(run.state.outcome?.commands).toEqual([
      TRIGGER_SUGGEST_COMMAND,
      ACCEPT_SUGGESTION_COMMAND,
    ]);
  });

  it("never accepts an item twice, however often the widget reports it focused", async () => {
    const editor = new SimulatedEditor({ insertsIdentifier: true, importEdit: "on-resolve" });
    const run = start(acceptCompletion(editor.deps()));
    await drain();

    editor.listArrives();
    editor.listArrives();
    await drain();
    editor.listArrives();
    editor.resolveArrives();
    await drain();

    expect(run.state.error).toBeUndefined();
    expect(editor.log.filter((entry) => entry === ACCEPT_SUGGESTION_COMMAND)).toHaveLength(1);
    expect(editor.log.filter((entry) => entry === TRIGGER_SUGGEST_COMMAND)).toHaveLength(1);
  });

  // Each failure is reached only through the hang guard: nothing in the editor ever
  // reaches the accepted state, and the driver must report that without issuing a
  // second trigger or accept to "try again".
  const failures: ReadonlyArray<{
    name: string;
    behaviour: AcceptBehaviour;
    listArrives: boolean;
    message: RegExp;
    commands: string[];
  }> = [
    {
      name: "an identifier change without the static import",
      behaviour: { insertsIdentifier: true, importEdit: "never" },
      listArrives: true,
      message: /document changed=true, import changed=false/,
      commands: [TRIGGER_SUGGEST_COMMAND, ACCEPT_SUGGESTION_COMMAND],
    },
    {
      name: "a no-op accept",
      behaviour: { insertsIdentifier: false, importEdit: "never" },
      listArrives: true,
      message: /document changed=false, import changed=false/,
      commands: [TRIGGER_SUGGEST_COMMAND, ACCEPT_SUGGESTION_COMMAND],
    },
    {
      name: "a suggestion list that never arrives",
      behaviour: { insertsIdentifier: true, importEdit: "with-insert" },
      listArrives: false,
      message: /never focused "computed", so no accept was issued/,
      commands: [TRIGGER_SUGGEST_COMMAND],
    },
  ];

  for (const failure of failures) {
    it(`fails ${failure.name}, with one trigger and no retry`, async () => {
      const editor = new SimulatedEditor(failure.behaviour);
      const run = start(acceptCompletion(editor.deps()));
      await drain();
      if (failure.listArrives) editor.listArrives();
      await drain();

      expect(run.state).toEqual({});
      editor.expireDeadline();
      await drain();

      expect(String(run.state.error)).toMatch(failure.message);
      expect(editor.log.filter((entry) => !entry.includes(":"))).toEqual(failure.commands);
      expect(editor.liveSubscriptions).toBe(0);
    });
  }

  it("releases every subscription and the deadline on success", async () => {
    const editor = new SimulatedEditor({ insertsIdentifier: true, importEdit: "with-insert" });
    const run = start(acceptCompletion(editor.deps()));
    await drain();
    expect(editor.liveSubscriptions).toBe(3);

    editor.listArrives();
    await drain();
    expect(run.state.error).toBeUndefined();
    expect(editor.liveSubscriptions).toBe(0);
    expect(editor.deadlineArmed).toBe(false);
  });

  it("releases every subscription when a command rejects", async () => {
    const editor = new SimulatedEditor({ insertsIdentifier: true, importEdit: "with-insert" });
    const failure = new Error("command service unavailable");
    const run = start(acceptCompletion(editor.deps({ runCommand: () => Promise.reject(failure) })));
    await drain();

    expect(run.state.error).toBe(failure);
    expect(editor.liveSubscriptions).toBe(0);
  });

  it("fails a command that never returns through the deadline, then releases everything", async () => {
    const editor = new SimulatedEditor({ insertsIdentifier: true, importEdit: "with-insert" });
    const run = start(
      acceptCompletion(editor.deps({ runCommand: () => new Promise<never>(() => undefined) })),
    );
    await drain();
    editor.expireDeadline();
    await drain();

    expect(String(run.state.error)).toMatch(/no accept was issued/);
    expect(editor.liveSubscriptions).toBe(0);
  });
});

/**
 * A fake `vscode` API slice with the two observation channels the adapter uses:
 * registered inline completion providers (called the way VS Code calls them when the
 * suggestion widget's focused item changes) and document-change listeners.
 */
function fakeVsCode(uri = "file:///w/src/AutoImportSymbol.vue") {
  let text = SCRIPT_BEFORE;
  const providers = new Set<vscode.InlineCompletionItemProvider>();
  const selectors: vscode.DocumentSelector[] = [];
  const changeListeners = new Set<(event: vscode.TextDocumentChangeEvent) => unknown>();
  const commands: string[] = [];
  const document = {
    uri: { scheme: "file", toString: () => uri },
    languageId: "vue",
    getText: () => text,
  } as unknown as vscode.TextDocument;
  const editor = { document } as unknown as vscode.TextEditor;
  const other = {
    uri: { scheme: "file", toString: () => "file:///w/src/Other.vue" },
  } as unknown as vscode.TextDocument;

  let onCommand: (command: string) => void = () => undefined;
  const api: VsCodeAcceptApi = {
    commands: {
      executeCommand: (command) => {
        commands.push(command);
        onCommand(command);
        return Promise.resolve(undefined);
      },
    },
    workspace: {
      onDidChangeTextDocument: (listener) => {
        changeListeners.add(listener);
        return { dispose: () => changeListeners.delete(listener) } as vscode.Disposable;
      },
    },
    languages: {
      registerInlineCompletionItemProvider: (selector, provider) => {
        selectors.push(selector);
        providers.add(provider);
        return { dispose: () => providers.delete(provider) } as vscode.Disposable;
      },
    },
  };

  const requestInline = (doc: vscode.TextDocument, selected: string | undefined) =>
    [...providers].map((provider) =>
      provider.provideInlineCompletionItems(
        doc,
        {} as vscode.Position,
        {
          triggerKind: 1,
          selectedCompletionInfo:
            selected === undefined ? undefined : { range: {} as vscode.Range, text: selected },
        } as vscode.InlineCompletionContext,
        {} as vscode.CancellationToken,
      ),
    );
  const edit = (next: string, doc: vscode.TextDocument = document) => {
    if (doc === document) text = next;
    for (const listener of [...changeListeners]) {
      listener({ document: doc } as vscode.TextDocumentChangeEvent);
    }
  };

  return {
    api,
    editor,
    other,
    commands,
    selectors,
    requestInline,
    edit,
    onCommand: (handler: (command: string) => void) => {
      onCommand = handler;
    },
    live: () => providers.size + changeListeners.size,
  };
}

describe("vscodeAcceptCompletionDeps", () => {
  const deadline = () => {
    let fire: (() => void) | undefined;
    return {
      armDeadline: (onExpired: () => void) => {
        fire = onExpired;
        return { dispose: () => (fire = undefined) };
      },
      fire: () => fire?.(),
    };
  };

  it("reports a focused suggestion only from the widget on the accepted document", () => {
    const vs = fakeVsCode();
    const deps = vscodeAcceptCompletionDeps(vs.api, vs.editor, {
      expectedSuggestion: "computed",
      armDeadline: deadline().armDeadline,
    });
    const seen: string[] = [];
    const subscription = deps.observeFocusedSuggestion((text) => seen.push(text));

    expect(vs.selectors).toEqual([{ language: "vue", scheme: "file" }]);
    // An ordinary inline request (no widget selection) and another document's widget
    // are not focus reports; the probe contributes no inline items in any case.
    expect(vs.requestInline(vs.editor.document, undefined)).toEqual([undefined]);
    expect(vs.requestInline(vs.other, "computed")).toEqual([undefined]);
    expect(seen).toEqual([]);
    expect(vs.requestInline(vs.editor.document, "computed")).toEqual([undefined]);
    expect(seen).toEqual(["computed"]);

    subscription.dispose();
    expect(vs.live()).toBe(0);
  });

  it("reports document changes only for the accepted document", () => {
    const vs = fakeVsCode();
    const deps = vscodeAcceptCompletionDeps(vs.api, vs.editor, {
      expectedSuggestion: "computed",
      armDeadline: deadline().armDeadline,
    });
    let changes = 0;
    const subscription = deps.observeDocumentChange(() => (changes += 1));

    vs.edit("", vs.other);
    expect(changes).toBe(0);
    vs.edit(SCRIPT_IDENTIFIER_ACCEPTED);
    expect(changes).toBe(1);

    subscription.dispose();
    expect(vs.live()).toBe(0);
  });

  it("drives one trigger and one accept through executeCommand, judged on the static imports", async () => {
    const vs = fakeVsCode();
    let focused = false;
    vs.onCommand((command) => {
      if (command === ACCEPT_SUGGESTION_COMMAND && focused) vs.edit(SCRIPT_IDENTIFIER_ACCEPTED);
    });
    const guard = deadline();
    const run = start(
      acceptCompletion(
        vscodeAcceptCompletionDeps(vs.api, vs.editor, {
          expectedSuggestion: "computed",
          armDeadline: guard.armDeadline,
        }),
      ),
    );
    await drain();
    expect(vs.commands).toEqual([TRIGGER_SUGGEST_COMMAND]);

    focused = true;
    vs.requestInline(vs.editor.document, "computed");
    await drain();
    expect(vs.commands).toEqual([TRIGGER_SUGGEST_COMMAND, ACCEPT_SUGGESTION_COMMAND]);
    // The whole `<script>` block changed, but its static imports did not.
    expect(run.state).toEqual({});

    vs.edit(SCRIPT_IMPORT_APPLIED);
    await drain();
    expect(run.state.error).toBeUndefined();
    expect(run.state.outcome?.importAfter).toBe('import { computed, ref } from "vue";');
    expect(vs.commands).toEqual([TRIGGER_SUGGEST_COMMAND, ACCEPT_SUGGESTION_COMMAND]);
    expect(vs.live()).toBe(0);
  });

  it("fails an identifier-only accept through the adapter", async () => {
    const vs = fakeVsCode();
    vs.onCommand((command) => {
      if (command === ACCEPT_SUGGESTION_COMMAND) vs.edit(SCRIPT_IDENTIFIER_ACCEPTED);
    });
    const guard = deadline();
    const run = start(
      acceptCompletion(
        vscodeAcceptCompletionDeps(vs.api, vs.editor, {
          expectedSuggestion: "computed",
          armDeadline: guard.armDeadline,
        }),
      ),
    );
    await drain();
    vs.requestInline(vs.editor.document, "computed");
    await drain();
    guard.fire();
    await drain();

    expect(String(run.state.error)).toMatch(/document changed=true, import changed=false/);
    expect(vs.live()).toBe(0);
  });
});
