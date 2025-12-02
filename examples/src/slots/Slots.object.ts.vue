<script lang="ts">
import { defineComponent, type SlotsType } from "vue";

export default defineComponent({
  name: "Slots",
  slots: Object as SlotsType<{
    default: { item: string };
    header: { title: string };
    footer: {};
  }>,
  data() {
    return {
      items: ["Item 1", "Item 2", "Item 3"],
    };
  },
});
</script>

<template>
  <div>
    <header>
      <slot name="header" :title="'My Title'" />
    </header>
    <main>
      <div v-for="item in items" :key="item">
        <slot :item="item" />
      </div>
    </main>
    <footer>
      <slot name="footer" />
    </footer>
  </div>
</template>
