<script lang="ts" generic="T extends 'foo'">
import type { PropType } from "vue";

export default {
  props: {
    items: {
      type: Array as () => T[],
      required: true,
    },

    getKey: {
      type: Function as PropType<(item: T) => string | number>,
      required: true,
    },
    getLabel: {
      type: Function as PropType<(item: T) => string>,
      required: true,
    },
  },
  computed: {
    orderedItems() {
      return this.items.sort((item1, item2) =>
        this.getLabel(item1).localeCompare(this.getLabel(item2))
      );
    },
  },

  methods: {
    getItemAtIndex(index: number): T | undefined {
      if (index < 0 || index >= this.orderedItems.length) {
        return undefined;
      }
      return this.orderedItems[index];
    },
  },
};
</script>

<template>
  <ol>
    <li v-for="item in orderedItems" :key="getKey(item)">
      {{ getLabel(item) }}
    </li>
  </ol>
</template>
