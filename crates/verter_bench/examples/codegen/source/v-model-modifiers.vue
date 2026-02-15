<template>
  <div>
    <!-- Basic v-model -->
    <input v-model="basicValue" placeholder="Basic v-model" />

    <!-- .lazy modifier - syncs on change instead of input -->
    <input v-model.lazy="lazyValue" placeholder="Lazy (syncs on change)" />

    <!-- .number modifier - casts to number -->
    <input v-model.number="numericValue" type="number" placeholder="Number" />

    <!-- .trim modifier - trims whitespace -->
    <input v-model.trim="trimmedValue" placeholder="Trimmed" />

    <!-- Combined modifiers -->
    <input v-model.lazy.trim="lazyTrimmed" placeholder="Lazy + Trim" />
    <input v-model.number.trim="numberTrimmed" type="number" placeholder="Number + Trim" />

    <!-- Textarea -->
    <textarea v-model.trim="textareaValue" placeholder="Textarea trimmed"></textarea>

    <!-- Select -->
    <select v-model.number="selectValue">
      <option value="1">One</option>
      <option value="2">Two</option>
      <option value="3">Three</option>
    </select>
  </div>
</template>

<script setup>
import { ref } from "vue";

const basicValue = ref("");
const lazyValue = ref("");
const numericValue = ref(0);
const trimmedValue = ref("");
const lazyTrimmed = ref("");
const numberTrimmed = ref(0);
const textareaValue = ref("");
const selectValue = ref(1);
</script>
