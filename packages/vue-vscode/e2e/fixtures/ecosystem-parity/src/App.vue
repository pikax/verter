<script setup lang="ts">
import { libHello } from "@/lib/helper";
import { useNuxtStyleFlag } from "#imports";

const msg = libHello("vue");
const flag = useNuxtStyleFlag();
</script>

<template>
  <main>
    <p>{{ msg }} {{ flag }}</p>
  </main>
</template>
