<template>
  <div>
    <!-- Basic custom directive (no value) -->
    <input v-focus />

    <!-- Custom directive with value -->
    <div v-tooltip="'Hello World'">Hover for tooltip</div>

    <!-- Custom directive with dynamic value -->
    <div v-tooltip="tooltipText">Dynamic tooltip</div>

    <!-- Custom directive with argument -->
    <div v-tooltip:top="'Top tooltip'">Top positioned</div>
    <div v-tooltip:bottom="'Bottom tooltip'">Bottom positioned</div>

    <!-- Custom directive with modifiers -->
    <div v-tooltip.show="'Always visible'">Always shown</div>

    <!-- Custom directive with argument and modifiers -->
    <div v-custom:arg.mod1.mod2="value">Full directive syntax</div>

    <!-- Custom directive with dynamic argument -->
    <div v-tooltip:[position]="'Dynamic position'">Dynamic arg</div>

    <!-- Multiple directives on same element -->
    <input v-focus v-tooltip="'Focus here'" />
  </div>
</template>

<script setup>
import { ref } from "vue";

const tooltipText = ref("Dynamic tooltip text");
const value = ref("directive value");
const position = ref("top");
</script>
