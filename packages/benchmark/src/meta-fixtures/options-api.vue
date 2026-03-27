<script lang="ts">
import { defineComponent } from "vue";

export default defineComponent({
  name: "OptionsCard",
  props: {
    title: {
      type: String,
      required: true,
    },
    count: {
      type: Number,
      default: 0,
    },
    items: {
      type: Array as () => string[],
      default: () => [],
    },
    variant: {
      type: String as () => "default" | "outlined" | "filled",
      default: "default",
    },
  },
  emits: {
    select: (item: string) => typeof item === "string",
    clear: () => true,
    "update:count": (value: number) => typeof value === "number",
  },
  expose: ["reset", "scrollTo"],
  data() {
    return {
      isExpanded: false,
    };
  },
  computed: {
    hasItems(): boolean {
      return this.items.length > 0;
    },
  },
  methods: {
    reset() {
      this.isExpanded = false;
      this.$emit("update:count", 0);
    },
    scrollTo(index: number) {
      const el = this.$refs[`item-${index}`] as HTMLElement | undefined;
      el?.scrollIntoView();
    },
    toggle() {
      this.isExpanded = !this.isExpanded;
    },
  },
});
</script>

<template>
  <div :class="['card', variant]">
    <h3 @click="toggle">{{ title }} ({{ count }})</h3>
    <div v-if="isExpanded">
      <slot name="header" />
      <ul v-if="hasItems">
        <li v-for="(item, i) in items" :key="i" :ref="`item-${i}`" @click="$emit('select', item)">
          {{ item }}
        </li>
      </ul>
      <slot name="actions" />
    </div>
  </div>
</template>

<style scoped>
.card {
  border: 1px solid #ccc;
  border-radius: 8px;
  padding: 16px;
}
.card.outlined {
  border-width: 2px;
}
.card.filled {
  background: #f5f5f5;
}
</style>
