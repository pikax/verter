<script setup lang="ts">
// Imported (locally-bound) component exposing typed event-handler props. The template
// event-typing path types a handler's `$event` / arrow param from
// `InstanceType<typeof EmitChild>["$props"]["onPick"]` (etc.).
defineProps<{
  onPick?: (item: { pickId: number; pickLabel: string }) => void;
  "onRow-change"?: (row: { rowKey: number; rowName: string }) => void;
}>();
</script>

<template>
  <div>emit child</div>
</template>
