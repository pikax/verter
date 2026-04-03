/**
 * Integration tests for the native lightweight type evaluator.
 *
 * These tests verify that the Rust evaluator produces correct types
 * through the ComponentMetaChecker compat path.
 * Requires @verter/native to be built.
 */
import { mkdtempSync } from "node:fs";
import { tmpdir } from "node:os";
import { afterEach, describe, it, expect } from "vitest";
import { resolve } from "node:path";
import { createCheckerByJson } from "./compat/checker.js";
import { normalizePath as runtimeNormalizePath, shutdownMetaRuntime } from "./runtime/index.js";

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
  return trackChecker(
    await createCheckerByJson(
      projectRoot,
      {},
      { runtimeMode: "dedicated", typeExpansionBackend: "verter" },
    ),
  );
}

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

  // TODO: publicInstance is not yet produced by the native Rust evaluator.
  // sfcBlocks may also be missing. Re-enable when the native evaluator surfaces
  // publicInstance members and SFC block metadata on the component-meta payload.
  it("surfaces public-instance and SFC block metadata on the native payload", async () => {
    const checker = await createRuntimeChecker("native-eval-native-breadth");

    checker.updateFile(
      "BreadthButton.vue",
      `<script lang="ts">
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
</i18n>`,
    );

    const nativeMeta = (checker as any)._session.getComponentMeta(
      resolve((checker as any).projectRoot ?? "", "BreadthButton.vue"),
    );
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
    // Schema should list the literal values
    expect(variant!.schema).toBeDefined();
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
    expect(slot?.type).toContain("LocalConfig");
    expect(slot?.schema).toBeDefined();
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
</script>
<template><input :value="model" /></template>`,
    );

    const meta = await checker.getComponentMeta("Input.vue");

    // Model should synthesize a prop and an event
    const modelProp = meta.props.find((p) => p.name === "modelValue");
    expect(modelProp).toBeDefined();

    const updateEvent = meta.events.find((e) => e.name === "update:modelValue");
    expect(updateEvent).toBeDefined();
  });

  // TODO: The native Rust evaluator does not yet expand chained indexed-access
  // types like Button["variants"]["color"] to their concrete union members.
  // Currently returns the unexpanded form "Button["variants"]["color"] | undefined".
  // Re-enable when the native type evaluator supports indexed-access expansion
  // through generic helper types.
  it("resolves chained indexed-access props from registry-materialized generic helpers", async () => {
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

    const activeColor = meta.props.find((prop) => prop.name === "activeColor");
    expect(activeColor).toBeDefined();
    expect(activeColor!.type).toContain("primary");
    expect(typeof activeColor!.schema).not.toBe("string");

    const ui = meta.props.find((prop) => prop.name === "ui");
    expect(ui).toBeDefined();
    expect(ui!.type).toContain("root");
    expect(typeof ui!.schema).not.toBe("string");
  });

  // TODO: typeRegistry entries do not yet carry rawType or declaration metadata
  // from the native Rust evaluator. Re-enable when the resolver populates
  // rawType (source text) and declaration (canonical source, span) on registry entries.
  it("preserves type-registry declaration metadata in live native payloads", async () => {
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
          typeExpansionBackend: "verter",
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
    );

    expect(nativeMeta?.typeRegistry?.[0]?.rawType).toContain("export interface Props");
    expect(nativeMeta?.typeRegistry?.[0]?.declaration?.canonicalSource).toBe(
      runtimeNormalizePath(resolve(projectRoot, "types.ts")),
    );
  });

  // TODO: Same indexed-access expansion gap as the chained indexed-access test.
  // The native evaluator returns unexpanded Button["variants"]["color"] instead
  // of the concrete "primary" | "secondary" union. Re-enable when the evaluator
  // supports mapped type + indexed-access expansion through Id<T> intersections.
  it("resolves finite mapped helper types through indexed access and Id intersections", async () => {
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

    const activeColor = meta.props.find((prop) => prop.name === "activeColor");
    expect(activeColor).toBeDefined();
    expect(activeColor!.type).toContain("primary");
    expect(typeof activeColor!.schema).not.toBe("string");

    const ui = meta.props.find((prop) => prop.name === "ui");
    expect(ui).toBeDefined();
    expect(ui!.type).toContain("base");
    expect(typeof ui!.schema).not.toBe("string");

    const slotUi = meta.props.find((prop) => prop.name === "slotUi");
    expect(slotUi).toBeDefined();
    expect(slotUi!.type).toContain("base");
    expect(typeof slotUi!.schema).not.toBe("string");

    const defaultSlot = meta.slots.find((slot) => slot.name === "default");
    expect(defaultSlot).toBeDefined();
    expect(defaultSlot!.type).toContain("base");
    expect(typeof defaultSlot!.schema).not.toBe("string");
  });

  // TODO: Same indexed-access expansion gap. The native evaluator does not expand
  // Button["variants"]["color"] when the ComponentConfig generic has an opaque
  // second type argument (MissingAppConfig). Re-enable when the evaluator handles
  // partial generic instantiation with opaque/missing type parameters.
  it("materializes registry refs with bound generic members despite opaque sibling args", async () => {
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

    const color = meta.props.find((prop) => prop.name === "color");
    expect(color).toBeDefined();
    expect(color!.type).toContain("primary");
    expect(color!.type).toContain("secondary");
    expect(typeof color!.schema).not.toBe("string");

    const ui = meta.props.find((prop) => prop.name === "ui");
    expect(ui).toBeDefined();
    expect(ui!.type).toContain("base");
    expect(ui!.type).toContain("label");
    expect(typeof ui!.schema).not.toBe("string");
  });

  it("materializes imported component-config helpers for compat display and schema output", async () => {
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
    const color = meta.props.find((prop) => prop.name === "color");
    expect(color).toBeDefined();
    expect(color!.type).toContain("neutral");
    expect(typeof color!.schema).not.toBe("string");

    const ui = meta.props.find((prop) => prop.name === "ui");
    expect(ui).toBeDefined();
    expect(ui!.type).toContain("base");
    expect(ui!.type).toContain("label");
    expect(typeof ui!.schema).not.toBe("string");

    const defaultSlot = meta.slots.find((slot) => slot.name === "default");
    expect(defaultSlot).toBeDefined();
    expect(defaultSlot!.type).toContain("base");
    expect(typeof defaultSlot!.schema).not.toBe("string");
  });

  it("materializes transitive imported registry aliases for nested indexed-access helpers", async () => {
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
    const avatar = meta.props.find((prop) => prop.name === "avatar");

    expect(avatar).toBeDefined();
    expect(avatar!.type).toContain("size");
    expect(avatar!.type).toContain('"sm"');
    expect(avatar!.type).not.toContain("graphNode(");

    const schemaText = JSON.stringify(avatar!.schema);
    expect(schemaText).toContain('\\"sm\\"');
    expect(schemaText).not.toContain("graphNode(");
  });

  it("resolves imported slot binding indexed-access helpers from raw slot contracts", async () => {
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

    const defaultSlot = meta.slots.find((slot) => slot.name === "default");
    expect(defaultSlot).toBeDefined();
    expect(defaultSlot!.type).toContain("base");
    expect(typeof defaultSlot!.schema).not.toBe("string");
  });
});
