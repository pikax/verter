/**
 * Integration tests for the native lightweight type evaluator.
 *
 * These tests verify that the Rust evaluator produces correct types
 * through the ComponentMetaChecker compat path.
 * Requires @verter/native to be built.
 */
import { describe, it, expect } from "vitest";
import { resolve } from "node:path";
import { createCheckerByJson } from "./compat/checker.js";

let nextProjectRootId = 1;

async function createRuntimeChecker(name = "native-eval") {
  return createCheckerByJson(
    resolve(process.env.TEMP ?? "/tmp", `${name}-${nextProjectRootId++}`),
    {},
  );
}

// =============================================================================
// Native evaluator: basic prop types via checker
// =============================================================================

describe("native evaluator integration", () => {
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
});
