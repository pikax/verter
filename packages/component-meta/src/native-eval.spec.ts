/**
 * Integration tests for the native lightweight type evaluator.
 *
 * These tests verify the NATIVE CONTRACT through the ComponentMetaChecker
 * compat path. Two distinct surfaces are asserted:
 *
 * - `meta._verter.props[].type` — the STRUCTURED `TypeDescriptor` the wire
 *   publishes. Publication is shallow by design: path-precise indexed-access
 *   terminals materialize concretely (e.g. a two-hop `T['a']['b']` landing on
 *   a literal union), while object-surface projections and imported aliases
 *   stay structured `indexedAccess` / `ref` carriers. The concrete value for
 *   a carrier resolves only under an explicit consumer demand, never eagerly
 *   at publication.
 * - `meta.props[].type` — the compat (Volar-interop) DISPLAY string. For
 *   shallow carriers it renders the authored symbolic form today; rendering
 *   the concrete union/object display is the deferred consumer-demand compat
 *   materialization feature (U14 compat-demand parity), tracked in
 *   `docs/better-implementation/b6-lane3.md`.
 *
 * Requires @verter/native to be built.
 */
import { mkdtempSync } from "node:fs";
import { tmpdir } from "node:os";
import { afterEach, describe, it, expect } from "vitest";
import { resolve } from "node:path";
import { createCheckerByJson } from "./compat/checker.js";
import { nativeTypeRegistryToMap } from "./native-component-meta.js";
import type { NativeComponentMetaResult } from "./native-component-meta.js";
import { shutdownMetaRuntime } from "./runtime/index.js";

let nextProjectRootId = 1;
const activeCheckers = new Set<{ close(): void }>();

function trackChecker<T extends { close(): void }>(checker: T): T {
  activeCheckers.add(checker);
  return checker;
}

async function settleNativeProject(): Promise<void> {
  await new Promise<void>((resolve) => setImmediate(resolve));
}

async function createRuntimeChecker(name = "native-eval") {
  const projectRoot = mkdtempSync(resolve(process.env.TEMP ?? tmpdir(), `${name}-`));
  return trackChecker(await createCheckerByJson(projectRoot, {}, { runtimeMode: "dedicated" }));
}

/**
 * `schema` is REQUIRED by the compat `PropertyMeta` / `SlotMeta` contract,
 * but the negative predicates historically used around it
 * (`typeof x !== "string"`, `JSON.stringify(x)` sentinel scans) all PASS
 * for `schema === undefined` — and `typeof null === "object"`, so a bare
 * definedness/typeof check also passes for `null` and an empty `{}`; a
 * regression dropping or emptying the field would slip through. Accept
 * ONLY the two structured `PropertyMetaSchema` forms: an ARRAY, or a
 * NON-NULL kind-discriminated schema object
 * (`kind ∈ {"enum","array","event","object"}` with a string `type`).
 * Everything else — undefined, null, `{}`, a bare string, a foreign
 * object — fails loudly. The negative sentinel/`typeof` checks stay as
 * SECONDARY assertions at each site.
 */
function expectStructuredSchema(schema: unknown): void {
  expect(schema).toBeDefined();
  expect(schema).not.toBeNull();
  expect(typeof schema).toBe("object");
  if (Array.isArray(schema)) {
    return;
  }
  const discriminated = schema as { kind?: unknown; type?: unknown };
  expect(["enum", "array", "event", "object"]).toContain(discriminated.kind);
  expect(typeof discriminated.type).toBe("string");
}

// =============================================================================
// Helper self-test — the assertion helper itself must DISCRIMINATE
// =============================================================================

describe("expectStructuredSchema (helper self-test)", () => {
  it("rejects undefined, null, empty objects, bare strings, and non-discriminated shapes", () => {
    // The compat contract REQUIRES `schema`; a dropped field (undefined),
    // a null (`typeof null === "object"`), an empty `{}`, and a foreign
    // object must ALL fail loudly — none may pass vacuously.
    expect(() => expectStructuredSchema(undefined)).toThrow();
    expect(() => expectStructuredSchema(null)).toThrow();
    expect(() => expectStructuredSchema({})).toThrow();
    expect(() => expectStructuredSchema("string")).toThrow();
    expect(() => expectStructuredSchema({ kind: "mystery", type: "x" })).toThrow();
    expect(() => expectStructuredSchema({ kind: "enum" })).toThrow();
    expect(() => expectStructuredSchema({ type: "x" })).toThrow();
  });

  it("accepts exactly the two structured schema forms: arrays and kind-discriminated objects", () => {
    expect(() => expectStructuredSchema(["'a'", "'b'"])).not.toThrow();
    expect(() =>
      expectStructuredSchema({ kind: "enum", type: "'a' | 'b'", schema: ["'a'", "'b'"] }),
    ).not.toThrow();
    expect(() => expectStructuredSchema({ kind: "array", type: "string[]" })).not.toThrow();
    expect(() =>
      expectStructuredSchema({ kind: "event", type: "(id: number): void" }),
    ).not.toThrow();
    expect(() => expectStructuredSchema({ kind: "object", type: "Foo", schema: {} })).not.toThrow();
  });
});

// =============================================================================
// Native evaluator: basic prop types via checker
// =============================================================================

describe("native evaluator integration", () => {
  afterEach(() => {
    for (const checker of activeCheckers) {
      checker.close();
    }
    activeCheckers.clear();
    shutdownMetaRuntime();
  });

  it("evaluates simple typed props", async () => {
    const checker = await createRuntimeChecker("native-eval-basic");

    checker.updateFile(
      "Button.vue",
      `<script setup lang="ts">
defineProps<{
  label: string
  count: number
  disabled?: boolean
}>()
</script>
<template><div /></template>`,
    );

    // Verter's native project/session path is async and stages imported helper
    // hydration after overlay upserts. Yield once before asserting the resulting
    // metadata shape instead of treating same-tick visibility as a contract.
    await settleNativeProject();

    const meta = await checker.getComponentMeta("Button.vue");

    const label = meta.props.find((p) => p.name === "label");
    expect(label).toBeDefined();
    expect(label!.type).toBe("string");

    const count = meta.props.find((p) => p.name === "count");
    expect(count).toBeDefined();
    expect(count!.type).toBe("number");

    const disabled = meta.props.find((p) => p.name === "disabled");
    expect(disabled).toBeDefined();
    expect(disabled!.required).toBe(false);

    // Negative: no phantom props
    expect(meta.props.length).toBe(3);
  });

  it("surfaces a direct root summary on the native payload", async () => {
    const checker = await createRuntimeChecker("native-eval-root-info");

    checker.updateFile(
      "RootInfoButton.vue",
      `<script setup lang="ts">
defineProps<{ label: string }>()
</script>
<template><button>{{ label }}</button></template>`,
    );

    const nativeMeta = (checker as any)._session.getComponentMeta(
      resolve((checker as any).projectRoot ?? "", "RootInfoButton.vue"),
    );

    expect(nativeMeta?.rootInfo).toEqual({
      kind: "single",
      targets: [{ kind: "nativeElement", elementIndex: 0, tag: "button" }],
    });
  });

  it("surfaces public-instance and SFC block metadata on the native payload", async () => {
    const checker = await createRuntimeChecker("native-eval-native-breadth");

    const source = `<script lang="ts">
export const legacy = true
</script>
<script setup lang="ts" generic="T extends string = string" attrs="ButtonAttrs">
defineProps<{ label: string }>()
defineSlots<{
  default(props: { item: number }): any
}>()
defineExpose({
  focus() {}
})
</script>
<template lang="html" data-layout="stack">
  <button>{{ label }}</button>
  <slot :item="1" />
</template>
<style scoped module="theme" lang="scss">
.primary { color: red; }
</style>
<i18n lang="json">
{ "label": "Button" }
</i18n>`;

    const absPath = resolve((checker as any).projectRoot ?? "", "BreadthButton.vue");
    // The sfcBlocks sidecar reads the host-side base parse state
    // (`current_eval_state`), so the SFC must be resident in the shared base
    // project — the same route the Rust `upsert_base` fixtures use. A
    // session-overlay-only file leaves the host base view empty and the
    // sidecar absent.
    (checker as any)._session.engine.nativeProject.upsertBase(absPath, source);
    checker.updateFile("BreadthButton.vue", source);

    const nativeMeta = (checker as any)._session.getComponentMeta(absPath);
    const compatMeta = await checker.getComponentMeta("BreadthButton.vue");

    expect(nativeMeta?.publicInstance?.members.map((member: any) => member.name)).toEqual([
      "$slots",
      "label",
      "focus",
    ]);
    expect(compatMeta.exposed.map((member) => member.name)).toEqual(["focus"]);
    expect(nativeMeta?.sfcBlocks).toMatchObject({
      script: {
        lang: "ts",
      },
      scriptSetup: {
        lang: "ts",
        generic: "T extends string = string",
        attrsType: "ButtonAttrs",
      },
      template: {
        lang: "html",
      },
      styles: [
        {
          index: 0,
          lang: "scss",
          scoped: true,
          isModule: true,
          moduleName: "theme",
        },
      ],
      custom: [
        {
          index: 0,
          blockType: "i18n",
          lang: "json",
        },
      ],
    });
    expect(nativeMeta?.sfcBlocks?.template?.attributes).toContainEqual({
      name: "data-layout",
      value: "stack",
    });
  });

  it("evaluates union literal props with schema", async () => {
    const checker = await createRuntimeChecker("native-eval-union");

    checker.updateFile(
      "Chip.vue",
      `<script setup lang="ts">
defineProps<{
  variant: "primary" | "secondary" | "danger"
}>()
</script>
<template><div /></template>`,
    );

    const meta = await checker.getComponentMeta("Chip.vue");
    const variant = meta.props.find((p) => p.name === "variant");
    expect(variant).toBeDefined();
    // Type string should contain the literals
    expect(variant!.type).toContain("primary");
    // Schema should list the literal values — structured, never a
    // missing/null/empty stand-in.
    expectStructuredSchema(variant!.schema);
  });

  it("evaluates interface-backed props", async () => {
    const checker = await createRuntimeChecker("native-eval-interface");

    checker.updateFile(
      "Form.vue",
      `<script setup lang="ts">
interface FormData {
  name: string
  email: string
}
defineProps<FormData>()
</script>
<template><div /></template>`,
    );

    const meta = await checker.getComponentMeta("Form.vue");

    const name = meta.props.find((p) => p.name === "name");
    const email = meta.props.find((p) => p.name === "email");
    expect(name).toBeDefined();
    expect(email).toBeDefined();
    expect(name!.type).toBe("string");
    expect(email!.type).toBe("string");

    // Negative: no extra props, no interface name as prop
    expect(meta.props.length).toBe(2);
    expect(meta.props.some((p) => p.name === "FormData")).toBe(false);
  });

  it("handles renamed import cycles in shallow alias hydration", async () => {
    const checker = await createRuntimeChecker("native-eval-import-cycle");

    checker.updateFile(
      "helpers.ts",
      `type Id<T> = T

type SlotInfo<T> = Id<{
  value: T
}>

type WithChildren<T> = {
  slot: SlotInfo<ComponentConfig<T>>
}

export type ComponentConfig<T> = WithChildren<T>
`,
    );

    checker.updateFile(
      "Button.vue",
      `<script lang="ts">
import type { ComponentConfig as LocalConfig } from './helpers'

export interface ButtonProps {
  slot?: LocalConfig<string>['slot']
}
</script>
<script setup lang="ts">
defineProps<ButtonProps>()
</script>
<template><div /></template>`,
    );

    const meta = await checker.getComponentMeta("Button.vue");
    const slot = meta.props.find((prop) => prop.name === "slot");

    expect(slot).toBeDefined();
    // The solver expands through the renamed import alias and resolves the
    // indexed access `LocalConfig<string>['slot']` → `SlotInfo<ComponentConfig<string>>`
    // → `{ value: ComponentConfig<string> }`.  With the optional `?`, it becomes
    // `{ value: ComponentConfig<string>; } | undefined`.
    expect(slot?.type).toContain("ComponentConfig");
    expect(slot?.type).toContain("undefined");
    expectStructuredSchema(slot?.schema);
  });

  it("evaluates emit types", async () => {
    const checker = await createRuntimeChecker("native-eval-emits");

    checker.updateFile(
      "Emitter.vue",
      `<script setup lang="ts">
defineEmits<{
  change: [value: string]
  submit: [data: { name: string }]
}>()
</script>
<template><div /></template>`,
    );

    const meta = await checker.getComponentMeta("Emitter.vue");
    expect(meta.events.length).toBe(2);

    const change = meta.events.find((e) => e.name === "change");
    expect(change).toBeDefined();

    const submit = meta.events.find((e) => e.name === "submit");
    expect(submit).toBeDefined();
  });

  it("publishes the real payload tuple for an imported property-style emit", async () => {
    const checker = await createRuntimeChecker("native-eval-imported-emit");

    checker.updateFile("emits.ts", `export interface ImportedEmits { save: [id: number] }\n`);
    checker.updateFile(
      "Imported.vue",
      `<script setup lang="ts">
import type { ImportedEmits } from './emits'
defineEmits<ImportedEmits>()
</script>
<template><div /></template>`,
    );

    const meta = await checker.getComponentMeta("Imported.vue");
    const save = meta._verter!.events.find((e) => e.name === "save");
    expect(save).toBeDefined();
    // The REAL tuple — the session-resolved closed payload is the authority;
    // never a fabricated `unknown` and never a typed output failure.
    expect(save!.payload).toEqual({
      kind: "tuple",
      elements: [{ kind: "primitive", name: "number" }],
      labels: ["id"],
    });

    // Imported == local: the same payload authored inline publishes the
    // IDENTICAL structured descriptor.
    checker.updateFile(
      "Local.vue",
      `<script setup lang="ts">
defineEmits<{ save: [id: number] }>()
</script>
<template><div /></template>`,
    );
    const local = await checker.getComponentMeta("Local.vue");
    const localSave = local._verter!.events.find((e) => e.name === "save");
    expect(localSave).toBeDefined();
    expect(save!.payload).toEqual(localSave!.payload);
  });

  it("publishes the real payload tuple for a cross-file call-signature emit", async () => {
    const checker = await createRuntimeChecker("native-eval-crossfile-callsig-emit");

    checker.updateFile(
      "events.ts",
      `export interface Row { id: number }\nexport interface Events { (e: 'save', value: Row): void }\n`,
    );
    checker.updateFile(
      "CrossFile.vue",
      `<script setup lang="ts">
import type { Events } from './events'
defineEmits<Events>()
</script>
<template><div /></template>`,
    );

    const meta = await checker.getComponentMeta("CrossFile.vue");
    const save = meta._verter!.events.find((e) => e.name === "save");
    expect(save).toBeDefined();
    // The CALLABLE-PARAMS replay materializes the real `[value: Row]` tuple:
    // the payload param stays the shallow resolvable `Row` reference — never
    // a fabricated `unknown`, never a semantic miss, never an output failure.
    expect(save!.payload).toEqual({
      kind: "tuple",
      elements: [{ kind: "ref", name: "Row" }],
      labels: ["value"],
    });
    expect(JSON.stringify(save!.payload)).not.toContain("unknown");
    expect(JSON.stringify(save!.payload)).not.toContain("semanticMiss");
  });

  it("evaluates slot bindings", async () => {
    const checker = await createRuntimeChecker("native-eval-slots");

    checker.updateFile(
      "List.vue",
      `<script setup lang="ts">
defineSlots<{
  default(props: { item: string; index: number }): any
}>()
</script>
<template><slot :item="'test'" :index="0" /></template>`,
    );

    const meta = await checker.getComponentMeta("List.vue");
    expect(meta.slots.length).toBeGreaterThanOrEqual(1);

    const defaultSlot = meta.slots.find((s) => s.name === "default");
    expect(defaultSlot).toBeDefined();
  });

  it("evaluates defineModel types", async () => {
    const checker = await createRuntimeChecker("native-eval-model");

    checker.updateFile(
      "Input.vue",
      `<script setup lang="ts">
const model = defineModel<string>()
const req = defineModel<number>('required', { required: true })
const def = defineModel<boolean>('defaulted', { default: false })
const flag = defineModel('flag', { default: false })
</script>
<template><input :value="model" /></template>`,
    );

    const meta = await checker.getComponentMeta("Input.vue");

    // ── Compat (Volar-interop) display surface: the `T | undefined`
    //    optional-model decoration is DERIVED at the compat layer from the
    //    typed `required` flag — pinned as exact display strings.
    const modelProp = meta.props.find((p) => p.name === "modelValue");
    expect(modelProp).toBeDefined();
    expect(modelProp!.type).toBe("string | undefined");
    expect(modelProp!.required).toBe(false);
    expect(modelProp!.default).toBeUndefined();

    const requiredProp = meta.props.find((p) => p.name === "required");
    expect(requiredProp).toBeDefined();
    expect(requiredProp!.type).toBe("number");
    expect(requiredProp!.required).toBe(true);
    expect(requiredProp!.default).toBeUndefined();

    const defaultedProp = meta.props.find((p) => p.name === "defaulted");
    expect(defaultedProp).toBeDefined();
    expect(defaultedProp!.type).toBe("boolean | undefined");
    expect(defaultedProp!.required).toBe(false);
    expect(defaultedProp!.default).toBe("false");

    // Untyped `defineModel('flag', { default: false })` threads the authored
    // default value the same way as the typed form.
    const flagProp = meta.props.find((p) => p.name === "flag");
    expect(flagProp).toBeDefined();
    expect(flagProp!.required).toBe(false);
    expect(flagProp!.default).toBe("false");

    // Every model synthesizes its update event.
    expect(meta.events.find((e) => e.name === "update:modelValue")).toBeDefined();
    expect(meta.events.find((e) => e.name === "update:required")).toBeDefined();
    expect(meta.events.find((e) => e.name === "update:defaulted")).toBeDefined();
    expect(meta.events.find((e) => e.name === "update:flag")).toBeDefined();

    // ── Native structured surface (`_verter.props`): the descriptors stay
    //    BARE `T` (never a synthesized `T | undefined` union) plus the typed
    //    `required` / `hasDefault` / `default` fields the compat display
    //    derives from — a defaulted model always carries its authored
    //    default VALUE text alongside the presence flag.
    const nativeProps = meta._verter!.props;

    const nativeModel = nativeProps.find((p) => p.name === "modelValue");
    expect(nativeModel).toBeDefined();
    expect(nativeModel!.type).toEqual({ kind: "primitive", name: "string" });
    expect(nativeModel!.required).toBe(false);
    expect(nativeModel!.hasDefault).toBe(false);
    expect(nativeModel!.default).toBeUndefined();

    const nativeRequired = nativeProps.find((p) => p.name === "required");
    expect(nativeRequired).toBeDefined();
    expect(nativeRequired!.type).toEqual({ kind: "primitive", name: "number" });
    expect(nativeRequired!.required).toBe(true);
    expect(nativeRequired!.hasDefault).toBe(false);
    expect(nativeRequired!.default).toBeUndefined();

    const nativeDefaulted = nativeProps.find((p) => p.name === "defaulted");
    expect(nativeDefaulted).toBeDefined();
    expect(nativeDefaulted!.type).toEqual({ kind: "primitive", name: "boolean" });
    expect(nativeDefaulted!.required).toBe(false);
    expect(nativeDefaulted!.hasDefault).toBe(true);
    expect(nativeDefaulted!.default).toBe("false");

    const nativeFlag = nativeProps.find((p) => p.name === "flag");
    expect(nativeFlag).toBeDefined();
    expect(nativeFlag!.required).toBe(false);
    expect(nativeFlag!.hasDefault).toBe(true);
    expect(nativeFlag!.default).toBe("false");

    // Negative: no native descriptor carries a decorated undefined arm.
    for (const prop of [nativeModel!, nativeRequired!, nativeDefaulted!]) {
      expect(prop.type.kind).not.toBe("union");
    }
  });

  // The native evaluator RESOLVES chained indexed access under explicit
  // demand; publication keeps the wire shallow by design. Path-precise
  // two-hop terminals (`Button['variants']['color']`) land as concrete
  // literal unions on the structured surface, while single-hop
  // object-surface projections (`Button['slots']`) stay structured
  // indexed-access carriers. The compat display string renders the authored
  // symbolic form until consumer-selected compat materialization lands
  // (U14 compat-demand parity).
  it("publishes chained indexed-access props from generic helpers path-precisely on the structured surface", async () => {
    const checker = await createRuntimeChecker("native-eval-generic-indexed-access");

    checker.updateFile(
      "types.ts",
      `export type ComponentVariants<TTheme> = {
  color: "primary" | "secondary"
  size: "sm" | "md"
}

export type ComponentSlots<TTheme> = {
  root?: {
    base: string
  }
}

export type ComponentConfig<TTheme> = {
  variants: ComponentVariants<TTheme>
  slots: ComponentSlots<TTheme>
}`,
    );
    checker.updateFile(
      "Button.vue",
      `<script setup lang="ts">
import type { ComponentConfig } from './types'
import theme from '#build/ui/button'

type Button = ComponentConfig<typeof theme>

defineProps<{
  activeColor?: Button['variants']['color']
  ui?: Button['slots']
}>()
</script>
<template><div /></template>`,
    );

    const meta = await checker.getComponentMeta("Button.vue");
    const nativeProps = meta._verter!.props;

    // Path-precise terminal materialization: the two-hop indexed access
    // resolves to the concrete literal union on the structured surface.
    // Optionality is carried separately on `required` — the union must NOT
    // grow a synthesized undefined arm, and must NOT stay an unresolved
    // indexed-access/unknown carrier.
    const activeColor = nativeProps.find((prop) => prop.name === "activeColor");
    expect(activeColor).toBeDefined();
    expect(activeColor!.type).toEqual({
      kind: "union",
      types: [
        { kind: "literal", value: "primary" },
        { kind: "literal", value: "secondary" },
      ],
    });
    expect(activeColor!.required).toBe(false);

    // Object-surface projection publishes SHALLOW: the structured carrier
    // keeps the exact indexed-access shape — it must NOT eagerly
    // materialize the slots object and must NOT drop the source reference.
    const ui = nativeProps.find((prop) => prop.name === "ui");
    expect(ui).toBeDefined();
    expect(ui!.type).toEqual({
      kind: "indexedAccess",
      objectType: { kind: "ref", name: "Button" },
      indexType: { kind: "literal", value: "slots" },
    });
    expect(ui!.required).toBe(false);

    // Compat schema derives from the structured surface: DEFINED and
    // structured first (a missing required schema must fail), then the
    // secondary never-a-bare-string sentinel.
    const compatActiveColor = meta.props.find((prop) => prop.name === "activeColor");
    expectStructuredSchema(compatActiveColor!.schema);
    expect(typeof compatActiveColor!.schema).not.toBe("string");
    const compatUi = meta.props.find((prop) => prop.name === "ui");
    expectStructuredSchema(compatUi!.schema);
    expect(typeof compatUi!.schema).not.toBe("string");
  });

  // Type-registry entries publish a structured shallow ref carrier for the
  // macro type argument. The pre-expansion source text (`rawType`) and the
  // resolved declaration routing metadata (canonicalSource/span/kind) are
  // consumer-demand metadata, not populated at shallow publication.
  it("publishes type-registry entries with structured shallow carriers in live native payloads", async () => {
    const projectRoot = mkdtempSync(
      resolve(
        process.env.TEMP ?? tmpdir(),
        `native-eval-type-registry-metadata-${nextProjectRootId++}-`,
      ),
    );
    const checker = trackChecker(
      await createCheckerByJson(
        projectRoot,
        {},
        {
          runtimeMode: "dedicated",
        },
      ),
    );

    checker.updateFile(
      "types.ts",
      `export interface Props {
  label: string
}`,
    );
    checker.updateFile(
      "Button.vue",
      `<script setup lang="ts">
import type { Props } from './types'

defineProps<Props>()
</script>
<template><div /></template>`,
    );

    const nativeMeta = (checker as any)._session.getComponentMeta(
      resolve(projectRoot, "Button.vue"),
    ) as NativeComponentMetaResult | null;
    expect(nativeMeta).not.toBeNull();

    // The registry carries exactly the macro type argument, with its
    // requested/resolved name identity echoed on the declaration stub.
    const entry = nativeMeta!.typeRegistry?.find((candidate) => candidate.name === "Props");
    expect(entry).toBeDefined();
    expect(entry!.declaration).toMatchObject({
      requestedName: "Props",
      resolvedName: "Props",
    });

    // The decoded registry descriptor is the structured shallow ref carrier —
    // it must NOT decay to an unknown carrier and must NOT eagerly
    // materialize the interface body at the wire.
    const decodedRegistry = nativeTypeRegistryToMap(nativeMeta!);
    expect(decodedRegistry?.get("Props")).toEqual({ kind: "ref", name: "Props" });

    // The macro payload itself resolved through the imported interface: the
    // published prop surface materialized `label: string` even though the
    // registry carrier stays shallow.
    const compatMeta = await checker.getComponentMeta("Button.vue");
    const label = compatMeta._verter!.props.find((prop) => prop.name === "label");
    expect(label).toBeDefined();
    expect(label!.type).toEqual({ kind: "primitive", name: "string" });
  });

  // Mapped-helper indexed access resolves path-precise terminals concretely
  // (`Button['variants']['color']` through the mapped `ComponentVariants`)
  // while closed-key mapped object surfaces (`Button['slots']` /
  // `Button['ui']`) publish as shallow structured ref carriers. Concrete
  // display/schema for the carriers is consumer-demand materialization
  // (U14 compat-demand parity).
  it("publishes finite mapped helper types with concrete indexed-access terminals and shallow surface carriers", async () => {
    const checker = await createRuntimeChecker("native-eval-mapped-helpers");

    checker.updateFile(
      "MappedButton.vue",
      `<script setup lang="ts">
type Id<T> = {} & { [P in keyof T]: T[P] }

type ComponentVariants<T extends { variants?: Record<string, Record<string, any>> }> = {
  [K in keyof T['variants']]: keyof T['variants'][K]
}

type ComponentSlots<T extends { slots?: Record<string, any> }> = Id<{
  [K in keyof T['slots']]?: string
}>

type ComponentUI<T extends { slots?: Record<string, any> }> = Id<{
  [K in keyof Required<T['slots']>]: (props?: Record<string, any>) => string
}>

type ComponentConfig<T extends Record<string, any>> = {
  variants: ComponentVariants<T>
  slots: ComponentSlots<T>
  ui: ComponentUI<T>
}

const theme = {
  variants: {
    color: { primary: '', secondary: '' },
    variant: { solid: '', soft: '' }
  },
  slots: {
    base: '',
    label: ''
  }
} as const

type Button = ComponentConfig<typeof theme>

defineProps<{
  activeColor?: Button['variants']['color']
  ui?: Button['slots']
  slotUi?: Button['ui']
}>()

defineSlots<{
  default(props: {
    ui: Button['ui']
  }): any
}>()
</script>
<template><div /></template>`,
    );

    const meta = await checker.getComponentMeta("MappedButton.vue");
    const nativeProps = meta._verter!.props;

    // Concrete path-precise terminal through the mapped variants helper.
    const activeColor = nativeProps.find((prop) => prop.name === "activeColor");
    expect(activeColor).toBeDefined();
    expect(activeColor!.type).toEqual({
      kind: "union",
      types: [
        { kind: "literal", value: "primary" },
        { kind: "literal", value: "secondary" },
      ],
    });
    expect(activeColor!.required).toBe(false);

    // Object-surface hops stop at the mapped-helper ref carrier — the
    // navigate hop resolved `Button['slots']` / `Button['ui']` to the
    // helper reference without eagerly materializing the mapped surface.
    const ui = nativeProps.find((prop) => prop.name === "ui");
    expect(ui).toBeDefined();
    expect(ui!.type).toEqual({
      kind: "ref",
      name: "ComponentSlots",
      typeArguments: [{ kind: "unknown", rawType: "typeof theme" }],
    });
    expect(ui!.required).toBe(false);

    const slotUi = nativeProps.find((prop) => prop.name === "slotUi");
    expect(slotUi).toBeDefined();
    expect(slotUi!.type).toEqual({
      kind: "ref",
      name: "ComponentUI",
      typeArguments: [{ kind: "unknown", rawType: "typeof theme" }],
    });
    expect(slotUi!.required).toBe(false);

    // The slot contract publishes its binding as a shallow synthetic
    // slot-binding carrier (graph-backed; never resolved through the
    // registry by name).
    const nativeDefaultSlot = meta._verter!.slots.find((slot) => slot.name === "default");
    expect(nativeDefaultSlot).toBeDefined();
    const uiBinding = nativeDefaultSlot!.bindings.find((binding) => binding.name === "ui");
    expect(uiBinding).toBeDefined();
    expect(uiBinding!.type).toMatchObject({
      kind: "syntheticSlotBinding",
      surfaceKind: "slotBinding",
      slotName: "default",
      bindingName: "ui",
    });

    // Compat schemas stay structured: DEFINED and structured first (a
    // missing required schema must fail), then the secondary
    // never-a-bare-string sentinel.
    for (const name of ["activeColor", "ui", "slotUi"]) {
      const compatProp = meta.props.find((prop) => prop.name === name);
      expect(compatProp).toBeDefined();
      expectStructuredSchema(compatProp!.schema);
      expect(typeof compatProp!.schema).not.toBe("string");
    }
    const compatDefaultSlot = meta.slots.find((slot) => slot.name === "default");
    expect(compatDefaultSlot).toBeDefined();
    expectStructuredSchema(compatDefaultSlot!.schema);
    expect(typeof compatDefaultSlot!.schema).not.toBe("string");
  });

  // A bound generic member resolves to its concrete union even when a
  // sibling type argument is opaque/unresolvable (`MissingAppConfig`); the
  // object-surface hop stays a shallow indexed-access carrier.
  it("resolves bound generic members to concrete unions despite opaque sibling args on the structured surface", async () => {
    const checker = await createRuntimeChecker("native-eval-opaque-registry-ref");

    checker.updateFile(
      "types.ts",
      `type ComponentVariants<T extends { variants?: Record<string, Record<string, any>> }> = {
  [K in keyof T['variants']]: keyof T['variants'][K]
}

type ComponentSlots<T extends { slots?: Record<string, any> }> = {
  [K in keyof T['slots']]?: string
}

export type ComponentConfig<T extends Record<string, any>, A> = {
  variants: ComponentVariants<T>
  slots: ComponentSlots<T>
  appConfig?: A
}`,
    );
    checker.updateFile(
      "theme.ts",
      `export default {
  variants: {
    color: { primary: '', secondary: '' }
  },
  slots: {
    base: '',
    label: ''
  }
} as const`,
    );
    checker.updateFile(
      "Button.vue",
      `<script lang="ts">
import type { ComponentConfig } from './types'
import theme from './theme'

type Button = ComponentConfig<typeof theme, MissingAppConfig>

export interface ButtonProps {
  color?: Button['variants']['color']
  ui?: Button['slots']
}
</script>
<script setup lang="ts">
defineProps<ButtonProps>()
</script>
<template><div /></template>`,
    );

    const meta = await checker.getComponentMeta("Button.vue");
    const nativeProps = meta._verter!.props;

    // The opaque `MissingAppConfig` sibling must not poison the bound
    // `typeof theme` member: the two-hop terminal still lands concrete.
    const color = nativeProps.find((prop) => prop.name === "color");
    expect(color).toBeDefined();
    expect(color!.type).toEqual({
      kind: "union",
      types: [
        { kind: "literal", value: "primary" },
        { kind: "literal", value: "secondary" },
      ],
    });
    expect(color!.required).toBe(false);

    const ui = nativeProps.find((prop) => prop.name === "ui");
    expect(ui).toBeDefined();
    expect(ui!.type).toEqual({
      kind: "indexedAccess",
      objectType: { kind: "ref", name: "Button" },
      indexType: { kind: "literal", value: "slots" },
    });
    expect(ui!.required).toBe(false);

    // Compat schemas stay structured: DEFINED and structured first (a
    // missing/null/empty required schema must fail), then the secondary
    // never-a-bare-string sentinel.
    const compatColor = meta.props.find((prop) => prop.name === "color");
    expectStructuredSchema(compatColor!.schema);
    expect(typeof compatColor!.schema).not.toBe("string");
    const compatUi = meta.props.find((prop) => prop.name === "ui");
    expectStructuredSchema(compatUi!.schema);
    expect(typeof compatUi!.schema).not.toBe("string");
  });

  it("resolves imported component-config helpers including app-config union arms on the structured surface", async () => {
    const checker = await createRuntimeChecker("native-eval-imported-component-config");

    checker.updateFile(
      "tailwind-variants.d.ts",
      `export type ClassValue = string | { [key: string]: boolean }
export type TVVariants<S, C, V> = { [K in keyof V]: keyof V[K] }
export type TVCompoundVariants<V, S, C, O, U> = never
export type TVDefaultVariants<V, S, O, U> = never`,
    );
    checker.updateFile(
      "tv.ts",
      `import type { ClassValue, TVVariants, TVCompoundVariants, TVDefaultVariants } from './tailwind-variants'

export type TVConfig<T extends Record<string, any>> = {
  [P in keyof T]?: {
    [K in keyof T[P] as K extends 'base' | 'slots' | 'variants' | 'defaultVariants' ? K : never]?: K extends 'base' ? ClassValue
      : K extends 'slots' ? {
        [S in keyof T[P]['slots']]?: ClassValue
      }
        : K extends 'variants' ? TVVariants<T[P]['slots'], ClassValue, WidenVariantsValues<T[P]['variants']>>
          : K extends 'defaultVariants' ? TVDefaultVariants<WidenVariantsValues<T[P]['variants']>, T[P]['slots'], object, undefined>
            : never
  }
} & {
  [P in keyof T]?: {
    compoundVariants?: TVCompoundVariants<WidenVariantsValues<T[P]['variants']>, T[P]['slots'], ClassValue, object, undefined>
  }
}

type WidenVariantsValues<V extends Record<string, any> | undefined>
  = V extends Record<string, any> ? V & {
    [K in keyof V]: V[K] extends Record<string, any>
      ? V[K] & Record<string & {}, any>
      : V[K]
  } : V

type Id<T> = {} & { [P in keyof T]: T[P] }

type ComponentVariants<T extends { variants?: Record<string, Record<string, any>> }> = {
  [K in keyof T['variants']]: keyof T['variants'][K]
}

type ComponentSlots<T extends { slots?: Record<string, any> }> = Id<{
  [K in keyof T['slots']]?: ClassValue
}>

type ComponentUI<T extends { slots?: Record<string, any> }> = Id<{
  [K in keyof Required<T['slots']>]: (props?: Record<string, any>) => string
}>

type GetComponentAppConfig<A, U extends string, K extends string>
  = A extends Record<U, Record<K, any>> ? A[U][K] : {}

type ComponentAppConfig<
  T,
  A extends Record<string, any>,
  K extends string,
  U extends string = 'ui' | 'ui.prose'
> = A & (
  U extends 'ui.prose'
    ? { ui?: { prose?: { [k in K]?: Partial<T> } } }
    : { [key in Exclude<U, 'ui.prose'>]?: { [k in K]?: Partial<T> } }
)

export type ComponentConfig<
  T extends Record<string, any>,
  A extends Record<string, any>,
  K extends string,
  U extends 'ui' | 'ui.prose' = 'ui'
> = {
  AppConfig: ComponentAppConfig<T, A, K, U>
  variants: ComponentVariants<T & GetComponentAppConfig<A, U, K>>
  slots: ComponentSlots<T>
  ui: ComponentUI<T>
}`,
    );
    checker.updateFile(
      "schema.ts",
      `export interface AppConfig {
  ui: {
    button: {
      variants: {
        color: {
          neutral: string
        }
      }
    }
  }
}`,
    );
    checker.updateFile(
      "theme.ts",
      `export default {
  variants: {
    color: { primary: '', secondary: '' },
    variant: { solid: '', soft: '' },
    size: { sm: '', md: '' }
  },
  slots: {
    base: '',
    label: ''
  }
} as const`,
    );
    checker.updateFile(
      "Button.vue",
      `<script lang="ts">
import type { AppConfig } from './schema'
import theme from './theme'
import type { ComponentConfig } from './tv'

type Button = ComponentConfig<typeof theme, AppConfig, 'button'>

export interface ButtonProps {
  color?: Button['variants']['color']
  ui?: Button['slots']
}

export interface ButtonSlots {
  default?(props: { ui: Button['ui'] }): any
}
</script>
<script setup lang="ts">
defineProps<ButtonProps>()
defineSlots<ButtonSlots>()
</script>
<template><div /></template>`,
    );

    await settleNativeProject();

    const meta = await checker.getComponentMeta("Button.vue");
    const nativeProps = meta._verter!.props;

    // The two-hop terminal merges the theme variants with the AppConfig
    // union arm (`GetComponentAppConfig` conditional): the structured union
    // must include the app-config-contributed `neutral` literal alongside
    // the theme literals.
    const color = nativeProps.find((prop) => prop.name === "color");
    expect(color).toBeDefined();
    expect(color!.type).toEqual({
      kind: "union",
      types: [
        { kind: "literal", value: "primary" },
        { kind: "literal", value: "secondary" },
        { kind: "literal", value: "neutral" },
      ],
    });
    expect(color!.required).toBe(false);

    const ui = nativeProps.find((prop) => prop.name === "ui");
    expect(ui).toBeDefined();
    expect(ui!.type).toEqual({
      kind: "indexedAccess",
      objectType: { kind: "ref", name: "Button" },
      indexType: { kind: "literal", value: "slots" },
    });
    expect(ui!.required).toBe(false);

    // Imported slot contract publishes a shallow synthetic binding carrier.
    const nativeDefaultSlot = meta._verter!.slots.find((slot) => slot.name === "default");
    expect(nativeDefaultSlot).toBeDefined();
    const uiBinding = nativeDefaultSlot!.bindings.find((binding) => binding.name === "ui");
    expect(uiBinding).toBeDefined();
    expect(uiBinding!.type).toMatchObject({
      kind: "syntheticSlotBinding",
      surfaceKind: "slotBinding",
      slotName: "default",
      bindingName: "ui",
    });

    // DEFINED and structured first (a missing required schema must fail),
    // then the secondary never-a-bare-string sentinel.
    const compatColor = meta.props.find((prop) => prop.name === "color");
    expectStructuredSchema(compatColor!.schema);
    expect(typeof compatColor!.schema).not.toBe("string");
    const compatUi = meta.props.find((prop) => prop.name === "ui");
    expectStructuredSchema(compatUi!.schema);
    expect(typeof compatUi!.schema).not.toBe("string");
    const compatDefaultSlot = meta.slots.find((slot) => slot.name === "default");
    expect(compatDefaultSlot).toBeDefined();
    expectStructuredSchema(compatDefaultSlot!.schema);
    expect(typeof compatDefaultSlot!.schema).not.toBe("string");
  });

  // A transitively imported alias prop publishes as a shallow structured ref
  // carrier — the nested indexed-access helpers behind `AvatarProps` resolve
  // only under explicit consumer demand.
  it("publishes transitive imported registry aliases as shallow ref carriers", async () => {
    const checker = await createRuntimeChecker("native-eval-transitive-registry");

    checker.updateFile(
      "tv.ts",
      `type ComponentVariants<T extends { variants?: Record<string, Record<string, any>> }> = {
  [K in keyof T['variants']]: keyof T['variants'][K]
}

type ComponentSlots<T extends { slots?: Record<string, any> }> = {} & {
  [K in keyof T['slots']]?: string
}

export type ComponentConfig<T extends Record<string, any>> = {
  variants: ComponentVariants<T>
  slots: ComponentSlots<T>
}`,
    );
    checker.updateFile(
      "avatar-theme.ts",
      `export default {
  variants: {
    size: { sm: '', md: '' }
  },
  slots: {
    base: ''
  }
} as const`,
    );
    checker.updateFile(
      "avatar-types.ts",
      `import type { ComponentConfig } from './tv'
import avatarTheme from './avatar-theme'

export type Avatar = ComponentConfig<typeof avatarTheme>

export interface AvatarProps {
  size?: Avatar['variants']['size']
  ui?: Avatar['slots']
}`,
    );
    checker.updateFile(
      "Button.vue",
      `<script lang="ts">
import type { AvatarProps } from './avatar-types'

export interface ButtonProps {
  avatar?: AvatarProps
}
</script>
<script setup lang="ts">
defineProps<ButtonProps>()
</script>
<template><div /></template>`,
    );

    const meta = await checker.getComponentMeta("Button.vue");

    // The imported alias stays the exact bare ref carrier — never an eager
    // materialization of the avatar surface, never an unknown decay.
    const nativeAvatar = meta._verter!.props.find((prop) => prop.name === "avatar");
    expect(nativeAvatar).toBeDefined();
    expect(nativeAvatar!.type).toEqual({ kind: "ref", name: "AvatarProps" });
    expect(nativeAvatar!.required).toBe(false);

    // The required schema stays structured (a missing/null/empty schema
    // must fail — the stringify sentinel below passes vacuously for
    // those), and neither the display string nor the schema may leak raw
    // graph-node placeholders.
    const avatar = meta.props.find((prop) => prop.name === "avatar");
    expect(avatar).toBeDefined();
    expect(avatar!.type).not.toContain("graphNode(");
    expectStructuredSchema(avatar!.schema);
    const schemaText = JSON.stringify(avatar!.schema);
    expect(schemaText).not.toContain("graphNode(");
  });

  // An imported raw slot contract publishes its binding as the exact shallow
  // indexed-access carrier over the imported helper (`Button['ui']` →
  // `ComponentConfig<typeof theme>['ui']`).
  it("publishes imported slot binding indexed-access helpers as structured carriers", async () => {
    const checker = await createRuntimeChecker("native-eval-imported-slot-bindings");

    checker.updateFile(
      "types.ts",
      `type Id<T> = {} & { [P in keyof T]: T[P] }

export type ComponentUI<T extends { slots?: Record<string, any> }> = Id<{
  [K in keyof Required<T['slots']>]: (props?: Record<string, any>) => string
}>

export type ComponentConfig<T extends Record<string, any>> = {
  ui: ComponentUI<T>
}`,
    );
    checker.updateFile(
      "theme.ts",
      `export const theme = {
  slots: {
    base: '',
    label: ''
  }
} as const`,
    );
    checker.updateFile(
      "button-types.ts",
      `import type { ComponentConfig } from './types'
import { theme } from './theme'

export type Button = ComponentConfig<typeof theme>

export interface ButtonSlots {
  default?(props: {
    ui: Button['ui']
  }): any
}`,
    );
    checker.updateFile(
      "ImportedSlotButton.vue",
      `<script setup lang="ts">
import type { ButtonSlots } from './button-types'

defineSlots<ButtonSlots>()
</script>
<template><div /></template>`,
    );

    const meta = await checker.getComponentMeta("ImportedSlotButton.vue");

    const nativeDefaultSlot = meta._verter!.slots.find((slot) => slot.name === "default");
    expect(nativeDefaultSlot).toBeDefined();
    const uiBinding = nativeDefaultSlot!.bindings.find((binding) => binding.name === "ui");
    expect(uiBinding).toBeDefined();
    // The binding keeps the exact structured indexed-access carrier over
    // the imported helper — it must NOT eagerly materialize the mapped ui
    // object and must NOT drop the helper reference.
    expect(uiBinding!.type).toEqual({
      kind: "indexedAccess",
      objectType: {
        kind: "ref",
        name: "ComponentConfig",
        typeArguments: [{ kind: "unknown", rawType: "typeof theme" }],
      },
      indexType: { kind: "literal", value: "ui" },
    });

    const defaultSlot = meta.slots.find((slot) => slot.name === "default");
    expect(defaultSlot).toBeDefined();
    // DEFINED and structured first (a missing required schema must fail),
    // then the secondary never-a-bare-string sentinel.
    expectStructuredSchema(defaultSlot!.schema);
    expect(typeof defaultSlot!.schema).not.toBe("string");
  });

  it("keeps realistic generic tabs helper routes structurally resolved with shallow surface carriers", async () => {
    const checker = await createRuntimeChecker("native-eval-realistic-tabs");

    checker.updateFile(
      "node_modules/reka-ui/index.d.ts",
      `export interface TabsRootProps<T> {
  defaultValue?: T
  modelValue?: T
  activationMode?: 'automatic' | 'manual'
  unmountOnHide?: boolean
}

export interface TabsRootEmits<T> {
  (e: 'update:modelValue', payload: T): void
}`,
    );
    checker.updateFile(
      "utils.ts",
      `export type DynamicSlotsKeys<Name extends string | undefined, Suffix extends string | undefined = undefined> = (
  Name extends string
    ? Suffix extends string
      ? Name | \`\${Name}-\${Suffix}\`
      : Name
    : never
)

export type DynamicSlots<
  T extends { slot?: string },
  Suffix extends string | undefined = undefined,
  ExtraProps extends object = {}
> = {
  [K in DynamicSlotsKeys<T['slot'], Suffix>]?: (
    props: { item: Extract<T, { slot: K extends \`\${infer Base}-\${Suffix}\` ? Base : K }> } & ExtraProps
  ) => any
}

export type NestedItem<T> = T extends Array<infer I> ? NestedItem<I> : T

type IsPrimitive<T> = T extends (string | number | boolean | symbol | bigint | null | undefined)
  ? true
  : false

type IsPlainObject<T> = IsPrimitive<T> extends true
  ? false
  : T extends readonly any[] | ((...args: any[]) => any)
    ? false
    : T extends object ? true
      : false

type DotPathKeys<T> = IsPlainObject<T> extends true
  ? {
      [K in keyof T & string]:
      IsPlainObject<NonNullable<T[K]>> extends true
        ? K | \`\${K}.\${DotPathKeys<NonNullable<T[K]>>}\`
        : K
    }[keyof T & string]
  : never

export type GetItemKeys<
  I,
  T extends NestedItem<I> = NestedItem<I>
> = (keyof Extract<T, object> & string) | DotPathKeys<Extract<T, object>>`,
    );
    checker.updateFile(
      "tv.ts",
      `type Id<T> = {} & { [P in keyof T]: T[P] }

type ComponentVariants<T extends { variants?: Record<string, Record<string, any>> }> = {
  [K in keyof T['variants']]: keyof T['variants'][K]
}

type ComponentSlots<T extends { slots?: Record<string, any> }> = Id<{
  [K in keyof T['slots']]?: string
}>

type ComponentUI<T extends { slots?: Record<string, any> }> = Id<{
  [K in keyof Required<T['slots']>]: (props?: Record<string, any>) => string
}>

export type ComponentConfig<T extends Record<string, any>> = {
  variants: ComponentVariants<T>
  slots: ComponentSlots<T>
  ui: ComponentUI<T>
}`,
    );
    checker.updateFile(
      "theme.ts",
      `export default {
  variants: {
    color: { primary: '', secondary: '' },
    variant: { pill: '', link: '' },
    size: { sm: '', md: '' },
    orientation: { horizontal: '', vertical: '' }
  },
  slots: {
    root: '',
    list: '',
    trigger: '',
    label: '',
    content: ''
  }
} as const`,
    );
    checker.updateFile(
      "Tabs.vue",
      `<script lang="ts">
import type { TabsRootProps, TabsRootEmits } from 'reka-ui'
import type { ComponentConfig } from './tv'
import type { DynamicSlots, GetItemKeys } from './utils'
import theme from './theme'

type Tabs = ComponentConfig<typeof theme>

export interface TabsItem {
  label?: string
  value?: string | number
  slot?: string
  nested?: {
    path?: string
  }
}

export interface TabsProps<T extends TabsItem = TabsItem> extends Pick<TabsRootProps<string | number>, 'defaultValue' | 'modelValue' | 'activationMode' | 'unmountOnHide'> {
  items?: T[]
  color?: Tabs['variants']['color']
  variant?: Tabs['variants']['variant']
  size?: Tabs['variants']['size']
  orientation?: Tabs['variants']['orientation']
  valueKey?: GetItemKeys<T>
  labelKey?: GetItemKeys<T>
  ui?: Tabs['slots']
}

export interface TabsEmits extends TabsRootEmits<string | number> {}

type SlotProps<T extends TabsItem> = (props: { item: T, index: number, ui: Tabs['ui'] }) => any

export type TabsSlots<T extends TabsItem = TabsItem> = {
  leading?: SlotProps<T>
  default?(props: { item: T, index: number }): any
  trailing?: SlotProps<T>
  content?: SlotProps<T>
} & DynamicSlots<T, undefined, { index: number, ui: Tabs['ui'] }>
</script>
<script setup lang="ts" generic="T extends TabsItem">
withDefaults(defineProps<TabsProps<T>>(), {
  defaultValue: '0',
  orientation: 'horizontal',
  unmountOnHide: true,
  valueKey: 'value',
  labelKey: 'label'
})
defineEmits<TabsEmits>()
defineSlots<TabsSlots<T>>()
</script>
<template><div /></template>`,
    );

    await settleNativeProject();

    const meta = await checker.getComponentMeta("Tabs.vue");
    const nativeMeta = (checker as any)._session.getComponentMeta(
      resolve((checker as any).projectRoot ?? "", "Tabs.vue"),
    );
    const nativeProps = meta._verter!.props;

    // Concrete path-precise terminal through the theme variants.
    const nativeColor = nativeProps.find((prop) => prop.name === "color");
    expect(nativeColor).toBeDefined();
    expect(nativeColor!.type).toEqual({
      kind: "union",
      types: [
        { kind: "literal", value: "primary" },
        { kind: "literal", value: "secondary" },
      ],
    });
    expect(nativeColor!.required).toBe(false);

    // The generic-dependent helper stays a shallow ref carrier over the
    // unbound SFC type parameter.
    const nativeValueKey = nativeProps.find((prop) => prop.name === "valueKey");
    expect(nativeValueKey).toBeDefined();
    expect(nativeValueKey!.type).toEqual({
      kind: "ref",
      name: "GetItemKeys",
      typeArguments: [{ kind: "ref", name: "T" }],
    });

    // The object-surface hop stays the exact shallow indexed-access carrier.
    const nativeUi = nativeProps.find((prop) => prop.name === "ui");
    expect(nativeUi).toBeDefined();
    expect(nativeUi!.type).toEqual({
      kind: "indexedAccess",
      objectType: { kind: "ref", name: "Tabs" },
      indexType: { kind: "literal", value: "slots" },
    });
    expect(nativeUi!.required).toBe(false);

    // STRUCTURED first: `JSON.stringify(undefined)` is `undefined` and
    // `JSON.stringify(null)` is `"null"`, so the sentinel scans below pass
    // vacuously for a MISSING/null/empty required schema.
    const color = meta.props.find((prop) => prop.name === "color");
    expect(color).toBeDefined();
    expectStructuredSchema(color!.schema);
    expect(JSON.stringify(color!.schema)).not.toContain("graphNode(");

    const valueKey = meta.props.find((prop) => prop.name === "valueKey");
    expect(valueKey).toBeDefined();
    expectStructuredSchema(valueKey!.schema);
    expect(JSON.stringify(valueKey!.schema)).not.toContain("never");

    const ui = meta.props.find((prop) => prop.name === "ui");
    expect(ui).toBeDefined();
    expectStructuredSchema(ui!.schema);
    expect(JSON.stringify(ui!.schema)).not.toContain("graphNode(");

    const nativeContentSlot = nativeMeta?.slots?.find((slot: any) => slot.name === "content");
    expect(nativeContentSlot).toBeDefined();
    expect(nativeContentSlot.bindings.map((binding: any) => binding.name)).toEqual([
      "item",
      "index",
      "ui",
    ]);

    const contentSlot = meta.slots.find((slot) => slot.name === "content");
    expect(contentSlot).toBeDefined();
    expect(contentSlot!.type).toContain("item");
    expect(contentSlot!.type).toContain("ui");
    expectStructuredSchema(contentSlot!.schema);
    expect(JSON.stringify(contentSlot!.schema)).not.toContain("graphNode(");

    const leadingSlot = meta.slots.find((slot) => slot.name === "leading");
    expect(leadingSlot).toBeDefined();
    expect(leadingSlot!.type).toContain("item");
    expect(leadingSlot!.type).toContain("ui");
  });
});
