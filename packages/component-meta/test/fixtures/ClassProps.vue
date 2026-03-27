<script setup lang="ts">
class UserModel {
  constructor(
    public id: number,
    public name: string,
  ) {}
  greet(): string {
    return `Hello ${this.name}`;
  }
}
defineProps<{ model: UserModel }>();
</script>

<template><div /></template>
