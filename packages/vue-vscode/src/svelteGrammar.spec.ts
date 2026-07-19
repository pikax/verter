import { readFileSync } from "node:fs";
import { createRequire } from "node:module";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { beforeAll, describe, expect, it } from "vitest";
import * as oniguruma from "vscode-oniguruma";
import {
  INITIAL,
  Registry,
  type IGrammar,
  type IRawGrammar,
  type StateStack,
} from "vscode-textmate";

// Grammar smoke test: tokenize a representative .svelte fixture through the
// contributed TextMate grammar (the exact file `contributes.grammars` ships)
// and assert the key scopes land — script/style embedding, Svelte block and
// special tags, interpolations, directive attributes, and markup tags.

const srcDir = fileURLToPath(new URL(".", import.meta.url));
const extensionDir = path.resolve(srcDir, "..");
const require = createRequire(import.meta.url);

function grammarPathFromManifest(): string {
  const pkg = JSON.parse(readFileSync(path.join(extensionDir, "package.json"), "utf8")) as {
    contributes?: { grammars?: Array<{ language?: string; path: string }> };
  };
  const entry = (pkg.contributes?.grammars ?? []).find((g) => g.language === "svelte");
  if (!entry) throw new Error("package.json contributes no svelte grammar");
  return path.join(extensionDir, entry.path);
}

// `loadWASM` may only run once per process — share one grammar across suites.
let grammarPromise: Promise<IGrammar> | undefined;

function getSvelteGrammar(): Promise<IGrammar> {
  grammarPromise ??= loadSvelteGrammar();
  return grammarPromise;
}

async function loadSvelteGrammar(): Promise<IGrammar> {
  const wasmPath = require.resolve("vscode-oniguruma/release/onig.wasm");
  await oniguruma.loadWASM(readFileSync(wasmPath).buffer as ArrayBuffer);
  const registry = new Registry({
    onigLib: Promise.resolve({
      createOnigScanner: (sources) => new oniguruma.OnigScanner(sources),
      createOnigString: (str) => new oniguruma.OnigString(str),
    }),
    loadGrammar: async (scopeName) => {
      if (scopeName !== "source.svelte") {
        // Embedded grammars (source.ts, source.css.scss, ...) are not under
        // test; stub them as empty so their host rules stay live (vscode-textmate
        // silently drops a rule whose include resolves to a missing grammar,
        // whereas real VS Code always ships these grammars).
        return { scopeName, patterns: [] } as unknown as IRawGrammar;
      }
      return JSON.parse(readFileSync(grammarPathFromManifest(), "utf8"));
    },
  });
  const grammar = await registry.loadGrammar("source.svelte");
  if (!grammar) throw new Error("failed to load source.svelte grammar");
  return grammar;
}

interface Token {
  text: string;
  scopes: string[];
}

function tokenize(grammar: IGrammar, source: string): Token[][] {
  const lines = source.split("\n");
  let state: StateStack = INITIAL;
  return lines.map((line) => {
    const { tokens, ruleStack } = grammar.tokenizeLine(line, state);
    state = ruleStack;
    return tokens.map((t) => ({
      text: line.substring(t.startIndex, t.endIndex),
      scopes: t.scopes,
    }));
  });
}

const FIXTURE = `<script lang="ts">
  import SvelteChild from "./SvelteChild.svelte";
  let count: number = 0;
  const items = [1, 2, 3];
</script>

<!-- markup comment -->
<main class="wrap">
  <h1>{count}</h1>
  <button on:click|preventDefault={increment} bind:value={count} class:active={count > 0}
    style:color={color} use:action transition:fade>
    press
  </button>
  <SvelteChild {count} {...rest} label="static {count} mixed" />
  {#if count > 0}
    <p>positive</p>
  {:else if count < 0}
    <p>negative</p>
  {:else}
    <p>zero</p>
  {/if}
  {#each items as item, i (item)}
    <span>{item}</span>
  {/each}
  {#await promise}
    waiting
  {:then value}
    {value}
  {:catch error}
    {error}
  {/await}
  {#snippet row(item)}
    <li>{item}</li>
  {/snippet}
  {@render row(1)}
  {@html rawHtml}
  {@const doubled = count * 2}
  {#key count}
    <span>rekeyed</span>
  {/key}
  {@debug count}
  <SvelteChild let:row={r} />
</main>

<style lang="scss">
  .wrap {
    color: red;
  }
</style>
`;

/** All tokens (across all lines) whose text contains `needle`. */
function tokensContaining(all: Token[][], needle: string): Token[] {
  return all.flat().filter((t) => t.text.includes(needle));
}

/** True when some token containing `needle` carries a scope matching `scope`. */
function hasScope(all: Token[][], needle: string, scope: string | RegExp): boolean {
  return tokensContaining(all, needle).some((t) =>
    t.scopes.some((s) => (typeof scope === "string" ? s === scope : scope.test(s))),
  );
}

describe("svelte TextMate grammar smoke (tokenizes the contributed grammar file)", () => {
  let all: Token[][];

  beforeAll(async () => {
    const grammar = await getSvelteGrammar();
    all = tokenize(grammar, FIXTURE);
  });

  it("scopes every token under source.svelte", () => {
    for (const line of all) {
      for (const token of line) {
        expect(token.scopes[0]).toBe("source.svelte");
      }
    }
  });

  it("embeds <script lang='ts'> content as source.ts", () => {
    expect(hasScope(all, "import SvelteChild", "source.ts")).toBe(true);
    expect(hasScope(all, "count: number", "source.ts")).toBe(true);
  });

  it("embeds <style lang='scss'> content as source.css.scss", () => {
    expect(hasScope(all, "color: red", "source.css.scss")).toBe(true);
  });

  it("scopes markup comments", () => {
    expect(hasScope(all, "markup comment", /^comment\.block/)).toBe(true);
  });

  it("scopes element and component tags", () => {
    expect(hasScope(all, "button", /^entity\.name\.tag/)).toBe(true);
    expect(hasScope(all, "SvelteChild", /^support\.class\.component\.svelte$/)).toBe(true);
  });

  it("scopes {#if} / {:else if} / {:else} / {/if} block keywords", () => {
    expect(hasScope(all, "#if", /^keyword\.control/)).toBe(true);
    expect(hasScope(all, ":else if", /^keyword\.control/)).toBe(true);
    expect(hasScope(all, ":else", /^keyword\.control/)).toBe(true);
    expect(hasScope(all, "/if", /^keyword\.control/)).toBe(true);
  });

  it("scopes {#each} with its `as` keyword and {/each}", () => {
    expect(hasScope(all, "#each", /^keyword\.control/)).toBe(true);
    expect(hasScope(all, "as", /^keyword\.control/)).toBe(true);
    expect(hasScope(all, "/each", /^keyword\.control/)).toBe(true);
  });

  it("scopes {#await} / {:then} / {:catch} / {/await}", () => {
    expect(hasScope(all, "#await", /^keyword\.control/)).toBe(true);
    expect(hasScope(all, ":then", /^keyword\.control/)).toBe(true);
    expect(hasScope(all, ":catch", /^keyword\.control/)).toBe(true);
    expect(hasScope(all, "/await", /^keyword\.control/)).toBe(true);
  });

  it("scopes {#snippet} / {/snippet} and {@render}", () => {
    expect(hasScope(all, "#snippet", /^keyword\.control/)).toBe(true);
    expect(hasScope(all, "/snippet", /^keyword\.control/)).toBe(true);
    expect(hasScope(all, "@render", /^keyword\.control/)).toBe(true);
  });

  it("scopes {@html} and {@const}", () => {
    expect(hasScope(all, "@html", /^keyword\.control/)).toBe(true);
    expect(hasScope(all, "@const", /^keyword\.control/)).toBe(true);
  });

  it("scopes {#key} / {/key} block keywords", () => {
    expect(hasScope(all, "#key", /^keyword\.control/)).toBe(true);
    expect(hasScope(all, "/key", /^keyword\.control/)).toBe(true);
  });

  it("scopes {@debug}", () => {
    expect(hasScope(all, "@debug", /^keyword\.control/)).toBe(true);
  });

  it("scopes the let: directive with its bound name", () => {
    expect(hasScope(all, "let", /^entity\.other\.attribute-name\.directive/)).toBe(true);
    expect(hasScope(all, "row", /^entity\.other\.attribute-name\.svelte$/)).toBe(true);
  });

  it("scopes {expression} interpolations as embedded expressions", () => {
    // `{count}` inside <h1> — the braces are interpolation punctuation and the
    // body is an embedded expression region.
    const h1Line = all.find((line) => line.some((t) => t.text.includes("h1")));
    expect(h1Line).toBeDefined();
    const open = h1Line!.find(
      (t) => t.text === "{" && t.scopes.some((s) => s.startsWith("punctuation.definition")),
    );
    expect(open, "interpolation `{` must carry a punctuation.definition scope").toBeDefined();
    expect(
      h1Line!.some(
        (t) => t.text.includes("count") && t.scopes.some((s) => s.includes("expression.embedded")),
      ),
    ).toBe(true);
  });

  it("scopes directive attributes (on:/bind:/class:/style:/use:/transition:) with modifiers", () => {
    for (const needle of ["on", "click", "bind", "value", "active", "color", "action", "fade"]) {
      expect(
        hasScope(all, needle, /^entity\.other\.attribute-name/),
        `directive part ${needle} must scope as an attribute name`,
      ).toBe(true);
    }
    expect(hasScope(all, "preventDefault", /^entity\.other\.attribute-name/)).toBe(true);
  });

  it("scopes shorthand and spread attributes as embedded expressions", () => {
    const componentLine = all.find((line) => line.some((t) => t.text.includes("...rest")));
    expect(componentLine).toBeDefined();
    const spread = componentLine!.find((t) => t.text.includes("...rest"));
    expect(spread!.scopes.some((s) => s.includes("expression.embedded"))).toBe(true);
    // Shorthand `{count}` in the same open tag is an embedded expression too.
    const shorthand = componentLine!.find(
      (t) => t.text === "count" && t.scopes.some((s) => s.includes("expression.embedded")),
    );
    expect(shorthand, "shorthand {count} must be an embedded expression").toBeDefined();
  });

  it("scopes interpolations inside quoted attribute values", () => {
    const mixedLine = all.find((line) => line.some((t) => t.text.includes("static")));
    expect(mixedLine).toBeDefined();
    const stringTokens = mixedLine!.filter((t) =>
      t.scopes.some((s) => s.startsWith("string.quoted")),
    );
    expect(stringTokens.length).toBeGreaterThan(0);
    // The `{count}` inside the quoted value opens an embedded expression.
    expect(
      mixedLine!.some(
        (t) =>
          t.scopes.some((s) => s.startsWith("string.quoted")) &&
          t.scopes.some((s) => s.includes("expression.embedded")),
      ),
    ).toBe(true);
  });

  it("does not leak markup scopes into the script region (negative)", () => {
    const scriptToken = tokensContaining(all, "count: number")[0];
    expect(scriptToken).toBeDefined();
    expect(scriptToken.scopes.some((s) => s.startsWith("entity.name.tag"))).toBe(false);
    expect(scriptToken.scopes.some((s) => s.includes("expression.embedded"))).toBe(false);
  });

  it("closes the script region at </script> (negative: style is not source.ts)", () => {
    expect(hasScope(all, "color: red", "source.ts")).toBe(false);
  });
});

// Every authored `lang` form must select the right embedded scope: double-quoted,
// single-quoted, and UNQUOTED values, plus the long-form aliases (`typescript`,
// `javascript`). Each case asserts the wanted scope AND the absence of the
// competing scope (a TS block mis-tokenized as source.js must fail).
describe("svelte TextMate grammar lang attribute forms", () => {
  let grammar: IGrammar;

  beforeAll(async () => {
    grammar = await getSvelteGrammar();
  });

  const scriptForms: Array<{ open: string; want: string; notWant: string }> = [
    { open: '<script lang="ts">', want: "source.ts", notWant: "source.js" },
    { open: "<script lang='ts'>", want: "source.ts", notWant: "source.js" },
    { open: "<script lang=ts>", want: "source.ts", notWant: "source.js" },
    { open: '<script lang="typescript">', want: "source.ts", notWant: "source.js" },
    { open: "<script lang='typescript'>", want: "source.ts", notWant: "source.js" },
    { open: "<script lang=typescript>", want: "source.ts", notWant: "source.js" },
    { open: "<script module lang=ts>", want: "source.ts", notWant: "source.js" },
    { open: '<script lang="js">', want: "source.js", notWant: "source.ts" },
    { open: "<script lang=js>", want: "source.js", notWant: "source.ts" },
    { open: '<script lang="javascript">', want: "source.js", notWant: "source.ts" },
    { open: "<script lang=javascript>", want: "source.js", notWant: "source.ts" },
    { open: "<script>", want: "source.js", notWant: "source.ts" },
  ];

  it.each(scriptForms)("embeds $open content as $want", ({ open, want, notWant }) => {
    const all = tokenize(grammar, `${open}\n  const marker = 1;\n</script>\n`);
    expect(hasScope(all, "const marker", want)).toBe(true);
    expect(hasScope(all, "const marker", notWant)).toBe(false);
  });

  const styleForms: Array<{ open: string; want: string }> = [
    { open: "<style lang=scss>", want: "source.css.scss" },
    { open: "<style lang='less'>", want: "source.css.less" },
    { open: "<style lang=postcss>", want: "source.css.postcss" },
  ];

  it.each(styleForms)("embeds $open content as $want", ({ open, want }) => {
    const all = tokenize(grammar, `${open}\n  .wrap { color: red; }\n</style>\n`);
    expect(hasScope(all, "color: red", want)).toBe(true);
    // Exact-scope negative: the generic css fallback must NOT have claimed it.
    expect(hasScope(all, "color: red", "source.css")).toBe(false);
  });

  it("does not activate TS embedding for lang values that merely start with ts (negative)", () => {
    const all = tokenize(grammar, `<script lang=tsx>\n  const marker = 1;\n</script>\n`);
    expect(hasScope(all, "const marker", "source.ts")).toBe(false);
  });
});
