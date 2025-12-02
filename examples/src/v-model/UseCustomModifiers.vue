<script setup lang="ts">
import { ref } from "vue";
import CustomModifiers from "./CustomModifiers.vue";

const text = ref("");
const title = ref("");
</script>

<template>
  <div>
    <h2>Custom v-model Modifiers</h2>

    <!-- Using custom modifiers on component -->
    <CustomModifiers v-model.capitalize="text" v-model:title.reverse="title" />

    <p>Text (capitalize): {{ text }}</p>
    <p>Title (reverse): {{ title }}</p>
  </div>
</template>
