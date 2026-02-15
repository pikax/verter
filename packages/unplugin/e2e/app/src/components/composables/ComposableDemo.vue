<script setup lang="ts">
import { useCounter } from '../../composables/useCounter'

const { count, doubled, increment, decrement, reset } = useCounter(0)
</script>

<template>
  <div data-testid="composable-demo">
    <span data-testid="composable-count">{{ count }}</span>
    <span data-testid="composable-doubled">{{ doubled }}</span>
    <button data-testid="composable-increment" @click="increment">+</button>
    <button data-testid="composable-decrement" @click="decrement">-</button>
    <button data-testid="composable-reset" @click="reset">Reset</button>
  </div>
</template>
