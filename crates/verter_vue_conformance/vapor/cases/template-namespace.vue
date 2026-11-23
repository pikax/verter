<template>
  <svg>
    <circle v-if="open" :r="radius" />
    <g v-else><rect /></g>
  </svg>
</template>
