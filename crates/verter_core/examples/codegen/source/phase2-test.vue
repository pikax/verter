<template>
  <div>
    <!-- Test 7: v-model on component -->
    <CustomInput v-model="inputValue" />

    <!-- Test 8: Event handler -->
    <button @click="handleClick(false)">Click</button>

    <!-- Test 9: Dynamic class -->
    <div :class="['static-class', { active: isActive }]">Dynamic class</div>

    <!-- Test 10: Context prefixing -->
    <span>{{ !isLoading && items.length }}</span>

    <!-- Test 12: Text content -->
    <p>Text: {{ message }}</p>

    <!-- Test 13: Component children -->
    <Container>
      <ChildComponent />
    </Container>
  </div>
</template>
<script setup lang="ts">
import { ref } from "vue";
import CustomInput from "./CustomInput.vue";
import Container from "./Container.vue";
import ChildComponent from "./ChildComponent.vue";

const inputValue = ref("");
const isActive = ref(true);
const isLoading = ref(false);
const items = ref([1, 2, 3]);
const message = ref("Hello");

function handleClick(value: boolean) {
  console.log("clicked", value);
}
</script>
