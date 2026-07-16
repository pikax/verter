<script setup lang="ts" generic="T">
/**
 * Advanced generic: T is inferred from `options` and flows into:
 * - props: modelValue: T
 * - events: update:modelValue / select payload T
 * - slots: option / selected slot props of type T
 * Call sites must NOT need GenericSelect&lt;string&gt;.
 */
const props = defineProps<{
  options: T[];
  modelValue: T;
  label?: string;
}>();

const emit = defineEmits<{
  "update:modelValue": [value: T];
  select: [value: T];
}>();

defineSlots<{
  option(props: { item: T; index: number }): any;
  selected(props: { value: T }): any;
}>();

function pick(v: T) {
  emit("update:modelValue", v);
  emit("select", v);
}
void props;
void pick;
</script>

<template>
  <div>
    <span v-if="label">{{ label }}</span>
    <slot name="selected" :value="modelValue" />
    <button v-for="(opt, i) in options" :key="i" type="button" @click="pick(opt)">
      <slot name="option" :item="opt" :index="i">{{ opt }}</slot>
    </button>
  </div>
</template>
