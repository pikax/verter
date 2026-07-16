<script setup lang="ts">
import EmitChild from "./EmitChild.vue";
const greeting = "hello";
function handle(n: number) {
  void n;
}
function consume(x: unknown) {
  void x;
}
</script>

<template>
  <p>{{ greeting }}</p>
  <button @click="(ev) => handle(ev.clientX)">inline arrow</button>
  <button @click="handle($event.clientX)">dollar event</button>
  <!-- Duplicate `@click`: the second handler routes through the spread path, where
       JSX contextual typing cannot flow — its `$event` must still be typed. -->
  <button @click="handle($event.clientY)" @click="handle($event.screenX)">duplicate spread</button>

  <!-- Local (imported-binding) component. Duplicate `@pick`: the second handler is a
       spread key, so its `$event` is the emit payload typed via
       InstanceType<typeof EmitChild>["$props"]["onPick"] — NOT `any`. -->
  <EmitChild @pick="consume($event.pickId)" @pick="consume($event.pickLabel)" />
  <!-- Local component, hyphenated emit → spread arrow param typed from the emit. -->
  <EmitChild @row-change="(row) => consume(row.rowKey)" />

  <!-- Global (GlobalComponents-augmented) component, NEVER imported. Duplicate `@ping`
       routes the second handler through the spread path; its `$event` resolves via the
       generated fallback const InstanceType<typeof GlobalEmitComp>. -->
  <GlobalEmitComp @ping="consume($event.pingCode)" @ping="consume($event.pingCount)" />
  <!-- Global component, hyphenated emit → spread arrow param via the fallback const. -->
  <GlobalEmitComp @late-signal="(sig) => consume(sig.sigName)" />
</template>
