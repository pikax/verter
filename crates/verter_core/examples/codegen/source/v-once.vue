<template>
  <div>
    <!-- v-once: render once and cache, skip future updates -->
    <span v-once>Static: {{ staticContent }}</span>

    <!-- v-once on element with children -->
    <div v-once>
      <h1>{{ title }}</h1>
      <p>{{ description }}</p>
    </div>

    <!-- v-once on component -->
    <MyComponent v-once :prop="value" />

    <!-- Regular reactive for comparison -->
    <span>Reactive: {{ reactiveContent }}</span>
  </div>
</template>

<script setup>
import { ref } from "vue";
import MyComponent from "./MyComponent.vue";

const staticContent = ref("This will not update");
const title = ref("Static Title");
const description = ref("Static Description");
const value = ref("static prop");
const reactiveContent = ref("This will update");
</script>
