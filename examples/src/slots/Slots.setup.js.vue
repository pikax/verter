<script setup>
// Slots using useSlots
import { useSlots } from "vue";

const slots = useSlots();
const items = ["Item 1", "Item 2", "Item 3"];
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
