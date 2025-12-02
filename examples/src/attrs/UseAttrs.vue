<script setup lang="ts">
import AttrsInheritance from "./AttrsInheritance.vue";
</script>

<template>
  <div>
    <h2>Attrs Inheritance Demo</h2>

    <!-- All these attrs will be forwarded to inner input -->
    <AttrsInheritance
      label="Username"
      type="text"
      placeholder="Enter username"
      class="custom-input"
      data-testid="username-input"
      :disabled="false"
      @input="(e) => console.log(e)"
      @focus="() => console.log('focused')"
    >
      <template #default>
        <span>Default slot content</span>
      </template>
      <template #helper>
        <span>Helper text</span>
      </template>
    </AttrsInheritance>
  </div>
</template>
