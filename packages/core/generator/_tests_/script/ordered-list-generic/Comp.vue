<script lang="ts" generic="T">
import { PropType, computed, defineComponent } from "vue";

export default defineComponent({
  props: {
    items: {
      type: Array as PropType<T[]>,
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

  setup(props, { expose }) {
    const orderedItems = computed<T[]>(() =>
      props.items.sort((item1, item2) =>
        props.getLabel(item1).localeCompare(props.getLabel(item2))
      )
    );

    function getItemAtIndex(index: number): T | undefined {
      if (index < 0 || index >= orderedItems.value.length) {
        return undefined;
      }
      return orderedItems.value[index];
    }

    expose({ getItemAtIndex });

    return {
      orderedItems,
      getItemAtIndex,
    };
  },
});
</script>

<template>
  <ol>
    <li v-for="item in orderedItems" :key="getKey(item)">
      {{ getLabel(item) }}
    </li>
  </ol>
</template>
