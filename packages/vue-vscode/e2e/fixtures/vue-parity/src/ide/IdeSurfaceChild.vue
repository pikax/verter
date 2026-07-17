<script setup lang="ts">
/**
 * Typed child for Ctrl+Click + completion E2E (props, emits, slots).
 */
defineProps<{
  label: string;
  count: number;
  enabled?: boolean;
  /** camelCase declare; template may use kebab `my-prop` */
  myProp?: string;
}>();

const emit = defineEmits<{
  pick: [value: string];
  change: [next: number];
  /** camelCase declare; template may use kebab `@my-event` */
  myEvent: [payload: string];
}>();

defineSlots<{
  header(props: { title: string; count: number }): any;
  default(props: { body: string }): any;
  /** camelCase declare; template may use kebab `#my-slot` */
  mySlot(props: { note: string }): any;
}>();

function fire() {
  emit("pick", "x");
  emit("change", 1);
}
</script>

<template>
  <button type="button" @click="fire">{{ label }}:{{ count }}</button>
  <header>
    <slot name="header" title="hdr" :count="count" />
  </header>
  <main>
    <slot body="main" />
  </main>
</template>
