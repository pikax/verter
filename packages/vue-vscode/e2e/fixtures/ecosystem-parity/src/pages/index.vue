<script setup lang="ts">
import { libHello } from "@/lib/helper";
import { useNuxtStyleFlag } from "#imports";

const pageTitle = libHello("page");
const flag = useNuxtStyleFlag();
</script>

<template>
  <main>
    <h1>{{ pageTitle }}</h1>
    <p>{{ flag }}</p>
  </main>
</template>
