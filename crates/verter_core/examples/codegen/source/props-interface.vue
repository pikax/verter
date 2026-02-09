<template>
  <div class="flex flex-col items-center justify-center">
    <div class="mt-[29px] text-[15px] text-white">{{ props.describeText }}</div>
    <div v-if="props.isTimeFilter" class="filter-button">选择时间</div>
  </div>
</template>
<script setup lang="ts">
interface Props {
  isTimeFilter?: boolean;
  describeText?: string;
}
const props = withDefaults(defineProps<Props>(), {
  isTimeFilter: false,
  describeText: "...",
});
</script>
