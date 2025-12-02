<script setup lang="ts">
import { inject } from "vue";
import { ThemeKey, CounterKey, NotifyKey } from "./keys";

// Deep nested component still has access to provided values
const theme = inject(ThemeKey);
const counter = inject(CounterKey);
const notify = inject(NotifyKey);

function notifyFromDeep() {
  notify?.("Message from deeply nested component!", "info");
}
</script>

<template>
  <div style="margin-left: 20px; border-left: 2px solid gray; padding-left: 10px">
    <h4>Deep Nested Component</h4>
    <p>Theme: {{ theme }}</p>
    <p>Counter: {{ counter }}</p>
    <button @click="notifyFromDeep">Notify from Deep</button>
  </div>
</template>
