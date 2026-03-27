<script setup lang="ts">
const count = 42;
const msg = "hello";
</script>
<template>
  <!-- PINNED: line 7, TS2304 — undefinedVar not defined -->
  <div>{{ undefinedVar }}</div>
  <!-- TS2339 — number has no property 'length' -->
  <div>{{ count.length }}</div>
  <!-- TS2345 — argument type mismatch -->
  <div>{{ count.toFixed("bad") }}</div>
  <!-- TS2362 — arithmetic on string -->
  <div>{{ msg * 5 }}</div>
</template>
