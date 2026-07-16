<script setup lang="ts">
import PropChild from "./PropChild.vue";

const contractProp = "from-parent";
</script>

<template>
  <!-- complete-here --><PropChild :contract-prop="contractProp" :optional-flag="true" />
</template>
