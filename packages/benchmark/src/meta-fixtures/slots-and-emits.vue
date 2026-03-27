<script setup lang="ts">
defineProps<{
  title: string;
  subtitle?: string;
  bordered: boolean;
}>();

defineEmits<{
  close: [];
  submit: [data: Record<string, unknown>];
  resize: [width: number, height: number];
  "update:visible": [visible: boolean];
}>();

defineSlots<{
  default(props: { isActive: boolean }): any;
  header(props: { title: string }): any;
  footer(props: { canSubmit: boolean; canClose: boolean }): any;
}>();
</script>

<template>
  <div :class="{ bordered }">
    <header>
      <slot name="header" :title="title">
        <h2>{{ title }}</h2>
        <p v-if="subtitle">{{ subtitle }}</p>
      </slot>
    </header>
    <main>
      <slot :is-active="true" />
    </main>
    <footer>
      <slot name="footer" :can-submit="true" :can-close="true" />
    </footer>
  </div>
</template>
