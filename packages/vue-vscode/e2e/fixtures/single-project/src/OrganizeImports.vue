<script setup lang="ts">
import { computed, ref } from "vue";
import MyComp from "./MyComp.vue";

const count = ref(0);
</script>

<template>
  <MyComp foo="literal" :bar="count" :baz="count" />
  <p>{{ count }}</p>
</template>
