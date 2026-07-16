<script setup lang="ts" generic="T = string">
/**
 * Defaulted generic: importers need not write GenericDefault&lt;string&gt;.
 * Unspecified T defaults to string.
 */
defineProps<{
  value?: T;
  prefix?: string;
}>();
</script>

<template>
  <span>{{ prefix }}{{ value }}</span>
</template>
