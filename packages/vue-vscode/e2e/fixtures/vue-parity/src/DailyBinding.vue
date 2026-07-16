<script setup lang="ts">
interface DailyValue {
  label: string;
  count: number;
}

const dailyValue: DailyValue = { label: "daily", count: 3 };

function renderDaily(): string {
  return `${dailyValue.label}:${dailyValue.count}`;
}
</script>

<template>
  <section>
    <button @click="renderDaily">{{ dailyValue.label }}</button>
    <span>{{ dailyValue.count }}</span>
  </section>
</template>
