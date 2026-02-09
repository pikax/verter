<script lang="ts" setup>
import { ref } from "vue";
defineProps<{
  name: string;
}>();

const test = ref(false);
</script>
<template>
  <div>
    <span @click.default>{{ name + test }}</span>

    <!-- <div v-if="test">foo {{ test }}</div> -->
  </div>
</template>
