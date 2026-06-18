#!/usr/bin/env node
/**
 * Generator — Svelte differential parity corpus (the GENERATED corpus).
 *
 * A DETERMINISTIC, systematic generator (NOT random fuzzing) that emits a
 * pairwise/combinatorial set of MINIMAL `.svelte` fixtures across the
 * topology-relevant axes (root kind, text, attributes, directives, events,
 * blocks/regions, namespace/special contexts), then compiles each with the
 * PINNED official `svelte@5.56.3` compiler and writes a NORMALIZED golden that
 * captures the EXPANDED topology schema the Rust differential matrix diffs
 * Verter's IR-derived candidate against.
 *
 * This is a sibling of `scripts/gen-svelte-goldens.mjs` (the hand-vendored
 * corpus). The two share the pinned-compiler loader + normalization primitives
 * (imported from `./svelte-golden-lib.mjs`) but own SEPARATE corpus subtrees:
 *   - hand-vendored fixtures  → `tests/svelte_oracle_corpus/{fixtures,goldens}`
 *   - generated fixtures      → `tests/svelte_oracle_corpus/{fixtures,goldens}/generated`
 *
 * ## Determinism
 *
 * Fixtures are produced from a STABLE, ordered axis enumeration (no timestamps,
 * no randomness, no `Date`, no PRNG). The fixture file names are derived from a
 * zero-padded ordinal + a slug of the case label, so re-running `--check` is
 * byte-stable. Goldens are written with 2-space JSON + a trailing newline (the
 * same serializer as the hand-vendored corpus).
 *
 * ## The expanded topology schema (client golden, generated corpus)
 *
 * On top of the hand-vendored fields (imports / exportDefault / helperSequence /
 * helperSet / helperCounts / delegatedEvents / templates / css) the generated
 * golden ALSO records the NORMALIZED differential axes:
 *   - `events`             — per registered event: `{type, target, delegation}`
 *                            where `target ∈ {element,window,document,body}` and
 *                            `delegation ∈ {delegated,direct,forwarded_prop,
 *                            attribute_effect}` (a component-targeted event is a
 *                            FORWARDED PROP; a spread is an attribute_effect).
 *   - `nonStaticProperties`— per "cannot be set statically" attr:
 *                            `{name, kind}` where `kind ∈ {autofocus,dom_property}`.
 *   - `attrParts`          — per dynamic/mixed attribute the EMITTED value-part
 *                            topology: `{helper, chunks:[literal|expr…]}` derived
 *                            from the official `$.set_*` / template-literal call.
 *   - `nodePaths`          — per client region, the NORMALIZED multiset of
 *                            node-path step sequences: each path is
 *                            `{base, steps:[…]}` with `base ∈ {fragment,node}` and
 *                            steps the official `$.first_child`/`$.child`/
 *                            `$.sibling`/`$.text` walk reduced to its STEP KINDS
 *                            (variable names + cursor-only `$.reset`/`$.next`
 *                            dropped). Order-independent within a region.
 *   - `dynamicSlots`       — the count of dynamic surfaces per slot KIND
 *                            (text/attribute/class/style/spread/bind/html/block/
 *                            event), a per-region-agnostic topology summary.
 *
 * Verter's IR-derived candidate produces the SAME normalized fields (a faithful
 * projection of the existing runtime IR), and the Rust matrix diffs them. Any
 * divergence is an automatically-failing test unless the `(fixture, axis)` pair
 * is on the honest `KNOWN_DIVERGENCES` allow-list.
 *
 * ## Usage
 *
 *     node scripts/gen-svelte-diff-corpus.mjs           # rewrite generated corpus
 *     node scripts/gen-svelte-diff-corpus.mjs --check   # assert in sync (CI gate)
 */

import {
  existsSync,
  mkdirSync,
  readdirSync,
  readFileSync,
  rmSync,
  writeFileSync,
} from "node:fs";
import { dirname, join, relative } from "node:path";
import { fileURLToPath } from "node:url";

import {
  extractClientDecodedText,
  extractClientDirectiveExprs,
  extractClientEvents,
  extractClientNodePaths,
  extractClientNonStaticProperties,
  extractDelegatedEvents,
  extractDynamicSlotCounts,
  extractExportDefault,
  extractImports,
  extractClientAttrParts,
  extractTemplates,
  helperCountsOf,
  helperSequenceOf,
  loadPinnedCompiler,
  normalizeCss,
  SVELTE_ORACLE_VERSION,
} from "./svelte-golden-lib.mjs";

const __dirname = dirname(fileURLToPath(import.meta.url));
const REPO_ROOT = join(__dirname, "..");
const CORPUS_ROOT = join(
  REPO_ROOT,
  "crates/verter_compiler/tests/svelte_oracle_corpus",
);
// The generated corpus lives under a `generated/` subtree of BOTH the fixtures
// and goldens dirs, so it is discovered by the same Rust walkers as the
// hand-vendored corpus while staying physically segregated (a regeneration of
// the generated corpus never rewrites a hand-vendored golden).
const GENERATED_FIXTURES_DIR = join(CORPUS_ROOT, "fixtures/generated");
const GENERATED_GOLDENS_DIR = join(CORPUS_ROOT, "goldens/generated");

const BACKENDS = ["client", "server"];

// ---------------------------------------------------------------------------
// The combinatorial fixture enumeration.
//
// Each axis is a list of VALUES; a value contributes a tiny markup fragment plus
// (optionally) a script snippet. We emit the pairwise (2-wise) combination set
// across the axes rather than the full cross-product — a deterministic greedy
// pairwise cover keeps the corpus to a few hundred minimal fixtures while still
// exercising every PAIR of axis values. Single-axis "focus" fixtures (one
// construct in isolation) are emitted first so each construct has a minimal
// standalone case, then the pairwise cover fills in cross-construct coverage.
// ---------------------------------------------------------------------------

/**
 * A construct fragment: a label, the markup it injects, and its STRUCTURAL
 * script needs. `kind` groups the construct by axis so the pairwise cover can
 * avoid pairing two values of the SAME axis (which would not be a meaningful
 * pair).
 *
 * Script needs are STRUCTURED so the composed component is runes-mode and the
 * reactive values stay reactive (the official compiler constant-FOLDS a plain
 * non-reactive `let x = 'v'` reference, which would silently turn a "dynamic
 * attribute" fixture into a static one). The columns:
 *   - `props` — reactive identifiers, declared via `let { … } = $props()` so a
 *               template reference is a live `$$props.x` read (NOT folded).
 *   - `comps` — component identifiers, declared via `import X from './X.svelte'`
 *               so a `<X/>` is a STATIC component reference (not a dynamic
 *               `$.component` wrapper a prop/local would force).
 *   - `funcs` — verbatim local declarations that do NOT fold (event handlers,
 *               action functions): `function ev() {}`.
 */
function frag(kind, label, markup, { props = [], comps = [], funcs = [] } = {}) {
  return { kind, label, markup, props, comps, funcs };
}

// Axis: ROOT KIND — the outermost template shape a construct sits in.
const ROOT_KINDS = [
  frag("root", "element", "<div>__SLOT__</div>"),
  frag("root", "component", "<Foo>__SLOT__</Foo>", { comps: ["Foo"] }),
  frag("root", "svelte_element", '<svelte:element this={tag}>__SLOT__</svelte:element>', {
    props: ["tag"],
  }),
  frag("root", "svelte_window", "<svelte:window __WINDOWATTR__/>__BARE__"),
  frag("root", "svelte_body", "<svelte:body __WINDOWATTR__/>__BARE__"),
  frag("root", "svelte_document", "<svelte:document __WINDOWATTR__/>__BARE__"),
  frag("root", "svelte_head", "<svelte:head><title>__SLOT__</title></svelte:head>"),
  frag("root", "svg", "<svg><rect />__SLOT__</svg>"),
  frag("root", "mathml", "<math><mrow>__SLOT__</mrow></math>"),
  frag("root", "if_block", "{#if cond}<p>__SLOT__</p>{/if}", { props: ["cond"] }),
  frag("root", "each_block", "{#each items as it}<p>__SLOT__</p>{/each}", {
    props: ["items"],
  }),
  frag("root", "await_block", "{#await pr}<p>load</p>{:then v}<p>__SLOT__</p>{/await}", {
    props: ["pr"],
  }),
  frag("root", "key_block", "{#key kk}<p>__SLOT__</p>{/key}", { props: ["kk"] }),
  frag("root", "snippet_block", "{#snippet row(z)}<li>__SLOT__</li>{/snippet}{@render row(1)}"),
];

// Axis: TEXT — text-content shapes that fill a `__SLOT__`. Dynamic shapes read a
// reactive prop so the official compiler emits a reactive text node (not folded).
const TEXTS = [
  frag("text", "static", "hello"),
  frag("text", "dynamic", "{tx}", { props: ["tx"] }),
  frag("text", "mixed", "a {tx} b", { props: ["tx"] }),
  frag("text", "named_entity", "&copy;"),
  frag("text", "numeric_entity", "&#65;"),
  frag("text", "hex_entity", "&#x41;"),
  frag("text", "numeric_no_semicolon", "&#65 z"),
  frag("text", "numeric_overflow", "&#9999999999;"),
];

// Axis: ATTRIBUTES — attribute shapes placed on an element. Dynamic / mixed /
// shorthand / spread values read reactive props (so they stay reactive).
const ATTRS = [
  frag("attr", "none", ""),
  frag("attr", "static", 'class="box"'),
  frag("attr", "dynamic", "id={aid}", { props: ["aid"] }),
  frag("attr", "mixed", 'class="a {acls} b"', { props: ["acls"] }),
  frag("attr", "boolean", "disabled"),
  frag("attr", "shorthand", "{value}", { props: ["value"] }),
  frag("attr", "shorthand_spaced", "{ value }", { props: ["value"] }),
  frag("attr", "spread", "{...rest}", { props: ["rest"] }),
];

// Axis: DIRECTIVES — class:/style:/bind:/use:/on: in unquoted + quoted forms.
// Each carries a `host` tag column (the element a host-constrained directive
// must sit on): `bind:value` requires `<input>`; the rest are host-agnostic
// (`<div>`). A `bind:value` target must be a WRITABLE binding (a `$state`, not a
// read-only prop) — declared via `funcs` as a `let dv = $state('')`. Directive
// condition/value reads are reactive props.
const dirFrag = (label, markup, needs, host = "div") => ({
  ...frag("dir", label, markup, needs),
  host,
});
const DIRECTIVES = [
  dirFrag("class_unquoted", "class:active={dx}", { props: ["dx"] }),
  dirFrag("class_quoted", 'class:active="{dx}"', { props: ["dx"] }),
  dirFrag("class_shorthand", "class:active", { props: ["active"] }),
  dirFrag("style_unquoted", "style:color={dx}", { props: ["dx"] }),
  dirFrag("style_quoted", 'style:color="{dx}"', { props: ["dx"] }),
  dirFrag("style_important", "style:color|important={dx}", { props: ["dx"] }),
  dirFrag("bind_value", "bind:value={dv}", { funcs: ["let dv = $state('');"] }, "input"),
  dirFrag("use_action", "use:act", { funcs: ["function act() {}"] }),
  dirFrag("use_action_arg", "use:act={dv}", { funcs: ["function act() {}"], props: ["dv"] }),
];

// Axis: EVENTS — delegated / non-delegated / capture / legacy on the chosen
// target. The handler is a local function (does not fold); these values supply
// the EVENT attribute text.
const EVENTS = [
  frag("event", "delegated_click", "onclick={ev}", { funcs: ["function ev() {}"] }),
  frag("event", "nondelegated_focus", "onfocus={ev}", { funcs: ["function ev() {}"] }),
  frag("event", "capture_click", "onclickcapture={ev}", { funcs: ["function ev() {}"] }),
  frag("event", "legacy_click", "on:click={ev}", { funcs: ["function ev() {}"] }),
  frag("event", "legacy_focus", "on:focus={ev}", { funcs: ["function ev() {}"] }),
  frag("event", "quoted_click", 'onclick="{ev}"', { funcs: ["function ev() {}"] }),
];

// ---------------------------------------------------------------------------
// Fixture assembly: each fixture composes a runes-mode script + a template from
// the chosen fragments' structural script needs. We keep markup minimal (one
// construct each).
// ---------------------------------------------------------------------------

/**
 * Compose a complete runes-mode fixture from the aggregated structural script
 * needs + a template body. `props` become one `let { … } = $props()` row,
 * `comps` become `import X from './X.svelte'` rows, `funcs` are emitted verbatim.
 * The aggregation is order-stable (sorted) so the composed source is
 * deterministic regardless of fragment-visit order.
 */
function composeFixture(needs, templateBody) {
  const props = [...new Set(needs.props ?? [])].sort();
  const comps = [...new Set(needs.comps ?? [])].sort();
  const funcs = [...new Set(needs.funcs ?? [])];
  const lines = [];
  for (const c of comps) lines.push(`import ${c} from './${c}.svelte';`);
  if (props.length > 0) lines.push(`let { ${props.join(", ")} } = $props();`);
  for (const f of funcs) lines.push(f);
  const script = lines.length > 0 ? `<script>\n  ${lines.join("\n  ")}\n</script>\n` : "";
  return `${script}${templateBody}\n`;
}

/** Merge the structural script needs of several fragments into one object. */
function mergeNeeds(...frags) {
  const out = { props: [], comps: [], funcs: [] };
  for (const f of frags) {
    if (!f) continue;
    out.props.push(...(f.props ?? []));
    out.comps.push(...(f.comps ?? []));
    out.funcs.push(...(f.funcs ?? []));
  }
  return out;
}

/**
 * A generated case: a stable label + the composed `.svelte` source. The label
 * becomes the fixture file name (ordinal-prefixed) and the golden slug.
 */
function makeCase(label, needs, templateBody) {
  return { label, source: composeFixture(needs, templateBody) };
}

/**
 * The DETERMINISTIC case enumeration. Returns an ordered list of
 * `{label, source}` — the order is fixed by the construction below, so file
 * names are stable across runs. `push(label, needs, body)` takes the merged
 * structural script needs (see `frag` / `mergeNeeds`).
 */
function enumerateCases() {
  const cases = [];
  const seenLabels = new Set();
  const push = (label, needs, templateBody) => {
    if (seenLabels.has(label)) {
      throw new Error(`duplicate generated-case label: ${label}`);
    }
    seenLabels.add(label);
    cases.push(makeCase(label, needs, templateBody));
  };

  // --- (A) ROOT-KIND focus: each root kind with a static text slot. ---
  for (const root of ROOT_KINDS) {
    const body = instantiateRoot(root, TEXTS[0], "");
    push(`root_${root.label}`, mergeNeeds(root, TEXTS[0]), body);
  }

  // --- (B) TEXT focus inside the two simplest carriers (element + if-block) ---
  // to exercise text axes in both a static-template and a block region.
  for (const carrier of [ROOT_KINDS[0], ROOT_KINDS[9]]) {
    for (const text of TEXTS) {
      const body = instantiateRoot(carrier, text, "");
      push(`text_${text.label}_in_${carrier.label}`, mergeNeeds(carrier, text), body);
    }
  }

  // --- (C) ATTRIBUTE focus on a plain element. ---
  for (const attr of ATTRS) {
    if (attr.label === "none") continue;
    push(`attr_${attr.label}`, mergeNeeds(attr), `<div ${attr.markup}>hi</div>`);
  }

  // --- (D) NON-STATIC PROPERTY focus (each on its proper host element). ---
  push("nsprop_video_muted", mergeNeeds(), "<video muted></video>");
  push("nsprop_input_autofocus", mergeNeeds(), "<input autofocus />");
  push("nsprop_input_default_value", mergeNeeds(), '<input defaultValue="x" />');
  push("nsprop_input_default_checked", mergeNeeds(), "<input defaultChecked />");

  // --- (E) DIRECTIVE focus on its host element (host-constraint-aware). ---
  for (const dir of DIRECTIVES) {
    push(`dir_${dir.label}`, mergeNeeds(dir), dirElement(dir, ""));
  }

  // --- (F) EVENT focus across each meaningful target. ---
  // element target:
  for (const ev of EVENTS) {
    push(`event_${ev.label}_on_element`, mergeNeeds(ev), `<button ${ev.markup}>x</button>`);
  }
  // component target (events become forwarded props in official); the component
  // is a STATIC imported reference so the call is `Foo($$anchor, { on…: … })`.
  const fooComp = frag("root", "foo", "", { comps: ["Foo"] });
  for (const ev of EVENTS) {
    if (ev.label.startsWith("legacy")) continue; // legacy on:* on a component is a distinct path
    push(
      `event_${ev.label}_on_component`,
      mergeNeeds(fooComp, ev),
      `<Foo ${ev.markup} />`,
    );
  }
  // svelte:element target:
  const tagProp = frag("root", "tag", "", { props: ["tag"] });
  for (const ev of [EVENTS[0], EVENTS[1], EVENTS[2]]) {
    push(
      `event_${ev.label}_on_svelte_element`,
      mergeNeeds(tagProp, ev),
      `<svelte:element this={tag} ${ev.markup}>x</svelte:element>`,
    );
  }
  // window / body / document targets (non-delegated specials):
  const evFn = { funcs: ["function ev() {}"] };
  push("event_window_resize", mergeNeeds(evFn), "<svelte:window onresize={ev} />");
  push("event_body_click", mergeNeeds(evFn), "<svelte:body onclick={ev} />");
  push("event_document_click", mergeNeeds(evFn), "<svelte:document onclick={ev} />");

  // --- (G) PAIRWISE cover across (root × attr), (root × event), (text × attr),
  // (attr × directive), (text × directive) — the 2-wise combination set,
  // filtered to avoid same-axis or structurally-invalid pairings. ---
  pairwiseCover(push, seenLabels);

  // --- (H) NAMESPACE / SPECIAL whitespace contexts. ---
  pushWhitespaceContexts(push);

  // --- (I) VALUE-FIDELITY focus: the directive / property value shapes that
  // exercise the value-chunk and directive-inner-expression axes. APPENDED last
  // (high ordinals) so the established 000-* fixture numbering is stable.
  pushValueFidelity(push);

  return cases;
}

/**
 * Emit the value-fidelity fixtures: a MIXED `defaultValue` (a non-static
 * property whose value is a literal/expr alternation), and the QUOTED
 * single-expression directive forms (`bind:value="{…}"` / `use:fn="{…}"` /
 * `onclick="{…}"`). These drive the non-static-property VALUE axis and the
 * directive-inner-expression axis — the cases where Verter's IR keeps the braces
 * (an object-literal) or collapses a mixed value to one expression.
 */
function pushValueFidelity(push) {
  // ROOT-level text-node seeds — a bare text root compiles to `$.text('seed')`
  // (the text-first region), so the DECODED-text seed is the comparison subject.
  // An ENTITY seed (`&copy;` / `&#65;` / a mixed `a &copy; b`) is DECODED by the
  // official compiler (`$.text('©')`) but kept RAW by Verter's `TextNode` seed —
  // the entity-decode divergence in the dynamic-text form. A plain ASCII seed
  // (`plain text`) decodes to itself (a no-divergence control).
  push("text_root_named_entity", {}, "&copy;");
  push("text_root_mixed_entity", {}, "a &copy; b");
  push("text_root_numeric_entity", {}, "&#65;");
  push("text_root_plain", {}, "plain text");
  // A MIXED `defaultValue` (`a {x} b`) — the value is a literal/expr alternation;
  // official keeps the alternation, Verter collapses it to a single expression.
  push(
    "nsprop_input_default_value_mixed",
    { props: ["x"] },
    '<input defaultValue="a {x} b" />',
  );
  // QUOTED single-expression directive forms. The `class:`/`style:` quoted forms
  // already exist (042/045); these add the bind / use / on families.
  push(
    "dir_bind_value_quoted",
    { funcs: ["let dv = $state('');"] },
    '<input bind:value="{dv}" />',
  );
  push(
    "dir_use_action_arg_quoted",
    { props: ["foo"], funcs: ["function act() {}"] },
    '<div use:act="{foo}">hi</div>',
  );
  push(
    "event_quoted_click_on_button",
    { funcs: ["function ev() {}"] },
    '<button onclick="{ev}">x</button>',
  );
}

/**
 * Instantiate a root-kind fragment with a chosen text + window-attr, returning
 * the markup body. Handles the `__SLOT__`, `__BARE__`, and `__WINDOWATTR__`
 * markers. (The chosen text's script needs are merged by the caller.)
 */
function instantiateRoot(root, text, windowAttr) {
  let body = root.markup;
  body = body.replaceAll("__SLOT__", text.markup);
  body = body.replaceAll("__BARE__", "");
  body = body.replaceAll("__WINDOWATTR__", windowAttr);
  return body;
}

/**
 * Emit the deterministic pairwise (2-wise) combination cover. A greedy
 * algorithm over a FIXED order: for each pair from two DISTINCT axes that is
 * structurally valid, emit one minimal fixture combining the two constructs.
 * Duplicate labels are skipped (a pair already covered by a focus fixture is not
 * re-emitted).
 */
function pairwiseCover(push, seenLabels) {
  // (root × attr): an attribute placed on the INNER element of the root template.
  const elementRoots = ROOT_KINDS.filter((r) =>
    ["element", "svg", "mathml", "if_block", "each_block", "key_block"].includes(r.label),
  );
  for (const root of elementRoots) {
    for (const attr of ATTRS) {
      if (attr.label === "none") continue;
      const body = attrOnRoot(root, attr);
      if (!body) continue;
      const label = `pair_root_${root.label}__attr_${attr.label}`;
      if (seenLabels.has(label)) continue;
      push(label, mergeNeeds(root, attr), body);
    }
  }

  // (text × attr): a dynamic/mixed attribute alongside a text shape on one
  // element — exercises attr-part topology with a sibling text slot.
  for (const text of [TEXTS[1], TEXTS[2], TEXTS[3]]) {
    for (const attr of [ATTRS[2], ATTRS[3], ATTRS[5]]) {
      const body = `<div ${attr.markup}>${text.markup}</div>`;
      const label = `pair_text_${text.label}__attr_${attr.label}`;
      if (seenLabels.has(label)) continue;
      push(label, mergeNeeds(text, attr), body);
    }
  }

  // (attr × directive): a static/dynamic attribute combined with a directive on
  // one element. The directive's host element is honored (a `bind:value` lands
  // on `<input>`).
  for (const attr of [ATTRS[1], ATTRS[2], ATTRS[3]]) {
    for (const dir of [DIRECTIVES[0], DIRECTIVES[3], DIRECTIVES[6]]) {
      const body = dirElement(dir, attr.markup);
      const label = `pair_attr_${attr.label}__dir_${dir.label}`;
      if (seenLabels.has(label)) continue;
      push(label, mergeNeeds(attr, dir), body);
    }
  }

  // (root × event): a delegated/non-delegated/capture event on the root element
  // inside each block root's body.
  const blockRoots = ROOT_KINDS.filter((r) =>
    ["if_block", "each_block", "key_block", "await_block"].includes(r.label),
  );
  for (const root of blockRoots) {
    for (const ev of [EVENTS[0], EVENTS[1], EVENTS[2]]) {
      const body = eventInBlock(root, ev);
      if (!body) continue;
      const label = `pair_root_${root.label}__event_${ev.label}`;
      if (seenLabels.has(label)) continue;
      push(label, mergeNeeds(root, ev), body);
    }
  }

  // (text × directive): a directive on an element wrapping a dynamic text. A
  // void-host directive (`bind:value` on `<input>`) has no text child, so only
  // host-agnostic (`<div>`) directives pair with a text body here.
  for (const text of [TEXTS[1], TEXTS[2]]) {
    for (const dir of [DIRECTIVES[0], DIRECTIVES[3], DIRECTIVES[6]]) {
      if (dir.host !== "div") continue;
      const body = `<div ${dir.markup}>${text.markup}</div>`;
      const label = `pair_text_${text.label}__dir_${dir.label}`;
      if (seenLabels.has(label)) continue;
      push(label, mergeNeeds(text, dir), body);
    }
  }
}

/**
 * Build a single element carrying a directive on its host element, plus any
 * extra attribute text. A `<input>` host is void (self-closing, no children); a
 * `<div>` host wraps a minimal `hi` body.
 */
function dirElement(dir, extraAttrs) {
  const attrs = [extraAttrs, dir.markup].filter((s) => s && s.length > 0).join(" ");
  if (dir.host === "input") {
    return `<input ${attrs} />`;
  }
  return `<div ${attrs}>hi</div>`;
}

/** Place an attribute on the inner element of a root-kind template. */
function attrOnRoot(root, attr) {
  switch (root.label) {
    case "element":
      return `<div ${attr.markup}>hi</div>`;
    case "svg":
      return `<svg><rect ${attr.markup} /></svg>`;
    case "mathml":
      return `<math><mrow ${attr.markup}>x</mrow></math>`;
    case "if_block":
      return `{#if cond}<p ${attr.markup}>hi</p>{/if}`;
    case "each_block":
      return `{#each items as it}<p ${attr.markup}>hi</p>{/each}`;
    case "key_block":
      return `{#key kk}<p ${attr.markup}>hi</p>{/key}`;
    default:
      return null;
  }
}

/** Place an event on the element inside a block root. */
function eventInBlock(root, ev) {
  switch (root.label) {
    case "if_block":
      return `{#if cond}<button ${ev.markup}>x</button>{/if}`;
    case "each_block":
      return `{#each items as it}<button ${ev.markup}>x</button>{/each}`;
    case "key_block":
      return `{#key kk}<button ${ev.markup}>x</button>{/key}`;
    case "await_block":
      return `{#await pr}<p>l</p>{:then v}<button ${ev.markup}>x</button>{/await}`;
    default:
      return null;
  }
}

/** Emit the namespace / special whitespace-context fixtures. */
function pushWhitespaceContexts(push) {
  push("ws_select", {}, "<select>\n  <option>a</option>\n  <option>b</option>\n</select>");
  push("ws_table", {}, "<table>\n  <tbody>\n    <tr><td>x</td></tr>\n  </tbody>\n</table>");
  push("ws_colgroup", {}, "<table>\n  <colgroup>\n    <col />\n  </colgroup>\n</table>");
  push("ws_datalist", {}, "<datalist>\n  <option value=\"a\"></option>\n</datalist>");
  push("ws_pre", {}, "<pre>\n  preserved\n</pre>");
  push("ws_textarea", {}, "<textarea>\n  preserved\n</textarea>");
  push("ws_pre_leading_newline", {}, "<pre>\nfirst</pre>");
  push("ws_svg_interior", {}, "<svg><rect /> <circle /></svg>");
  push("ws_svg_title", {}, "<svg><title>  t  </title></svg>");
  push("ws_svg_anchor", {}, '<svg><a href="x"><rect /></a></svg>');
  push("ws_adjacent_roots_space", {}, "<span>a</span> <span>b</span>");
  push("ws_adjacent_roots_nospace", {}, "<span>a</span><span>b</span>");
}

// ---------------------------------------------------------------------------
// Golden normalization for the GENERATED corpus (the EXPANDED schema).
// ---------------------------------------------------------------------------

/**
 * Normalize one compiled backend output into the generated-corpus golden. The
 * shared hand-vendored fields come from `svelte-golden-lib.mjs`; the EXPANDED
 * differential fields are client-only (the server backend has no DOM walk /
 * delegated set).
 */
function normalize(slug, backend, compiled) {
  const code = compiled.js.code;
  const css = normalizeCss(compiled);
  const helperSequence = helperSequenceOf(code);
  const helperSet = [...new Set(helperSequence)].sort();
  const helperCounts = helperCountsOf(helperSequence);

  const golden = {
    slug,
    backend,
    oracleVersion: SVELTE_ORACLE_VERSION,
    imports: extractImports(code),
    exportDefault: extractExportDefault(code),
    helperSequence,
    helperSet,
    helperCounts,
    delegatedEvents: backend === "client" ? extractDelegatedEvents(code) : [],
    templates: backend === "client" ? extractTemplates(code) : [],
    css,
  };

  if (backend === "client") {
    // The EXPANDED differential axes (client only).
    golden.events = extractClientEvents(code);
    golden.nonStaticProperties = extractClientNonStaticProperties(code);
    golden.attrParts = extractClientAttrParts(code);
    golden.directiveExprs = extractClientDirectiveExprs(code);
    golden.decodedText = extractClientDecodedText(code);
    golden.nodePaths = extractClientNodePaths(code);
    golden.dynamicSlots = extractDynamicSlotCounts(code);
  }

  return golden;
}

function serializeGolden(obj) {
  return JSON.stringify(obj, null, 2) + "\n";
}

function fixtureFileName(ordinal, label) {
  const padded = String(ordinal).padStart(3, "0");
  return `${padded}_${label}.svelte`;
}

function goldenPathFor(goldensDir, fixtureName, backend) {
  const rel = fixtureName.replace(/\.svelte$/, "");
  return join(goldensDir, `${rel}.${backend}.json`);
}

/** Build every fixture + golden into in-memory maps keyed by path. */
function buildCorpus(compiler) {
  const cases = enumerateCases();
  const fixtures = new Map(); // absolute fixture path -> source
  const goldens = new Map(); // absolute golden path -> serialized JSON

  cases.forEach((c, i) => {
    const fixtureName = fixtureFileName(i, c.label);
    const fixturePath = join(GENERATED_FIXTURES_DIR, fixtureName);
    fixtures.set(fixturePath, c.source);
    const slug = `generated/${fixtureName}`;
    for (const backend of BACKENDS) {
      let compiled;
      try {
        compiled = compiler.compile(c.source, {
          generate: backend,
          filename: slug,
          name: fixtureName.replace(/\.svelte$/, ""),
        });
      } catch (err) {
        throw new Error(
          `svelte compile failed for generated ${slug} (${backend}): ${err.message}\n--- source ---\n${c.source}`,
        );
      }
      const golden = normalize(slug, backend, compiled);
      goldens.set(
        goldenPathFor(GENERATED_GOLDENS_DIR, fixtureName, backend),
        serializeGolden(golden),
      );
    }
  });

  return { fixtures, goldens, count: cases.length };
}

function writeMode(compiler) {
  const { fixtures, goldens, count } = buildCorpus(compiler);
  // Clean + rewrite ONLY the generated subtrees (never the hand-vendored corpus).
  rmSync(GENERATED_FIXTURES_DIR, { recursive: true, force: true });
  rmSync(GENERATED_GOLDENS_DIR, { recursive: true, force: true });
  const all = [...fixtures, ...goldens].sort((a, b) => (a[0] < b[0] ? -1 : 1));
  for (const [path, content] of all) {
    mkdirSync(dirname(path), { recursive: true });
    writeFileSync(path, content);
  }
  console.log(
    `gen-svelte-diff-corpus: wrote ${count} generated fixture(s) + ${goldens.size} golden(s) ` +
      `from svelte@${SVELTE_ORACLE_VERSION} into ${relative(REPO_ROOT, CORPUS_ROOT)}/{fixtures,goldens}/generated`,
  );
}

function walkFiles(dir) {
  const out = [];
  if (!existsSync(dir)) return out;
  const walk = (d) => {
    for (const e of readdirSync(d, { withFileTypes: true }).sort((a, b) =>
      a.name < b.name ? -1 : a.name > b.name ? 1 : 0,
    )) {
      const p = join(d, e.name);
      if (e.isDirectory()) walk(p);
      else out.push(p);
    }
  };
  walk(dir);
  out.sort();
  return out;
}

function checkMode(compiler) {
  const { fixtures, goldens } = buildCorpus(compiler);
  const drift = [];

  const fresh = new Map([...fixtures, ...goldens]);
  // 1. Every fresh artifact must exist on-disk and be byte-equal.
  for (const [path, content] of fresh) {
    const rel = relative(REPO_ROOT, path);
    if (!existsSync(path)) {
      drift.push(`MISSING generated artifact: ${rel}`);
      continue;
    }
    const committed = readFileSync(path, "utf8");
    if (committed !== content) {
      drift.push(`DRIFTED generated artifact (on-disk != regenerated): ${rel}`);
    }
  }
  // 2. No stale orphan files under the generated subtrees.
  for (const dir of [GENERATED_FIXTURES_DIR, GENERATED_GOLDENS_DIR]) {
    for (const path of walkFiles(dir)) {
      if (!fresh.has(path)) {
        drift.push(`STALE generated artifact (no fresh source): ${relative(REPO_ROOT, path)}`);
      }
    }
  }

  if (drift.length > 0) {
    console.error(
      `gen-svelte-diff-corpus --check: ${drift.length} drift(s) detected:\n` +
        drift.map((d) => `  - ${d}`).join("\n") +
        `\n\nRun \`node scripts/gen-svelte-diff-corpus.mjs\` to regenerate.`,
    );
    process.exit(1);
  }
  console.log(
    `gen-svelte-diff-corpus --check: ${fresh.size} generated artifact(s) in sync with svelte@${SVELTE_ORACLE_VERSION}.`,
  );
}

function main() {
  const check = process.argv.includes("--check");
  const compiler = loadPinnedCompiler(REPO_ROOT);
  if (check) {
    checkMode(compiler);
  } else {
    writeMode(compiler);
  }
}

main();
