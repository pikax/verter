<template>
  <component :is="current" class="control">
    <slot></slot>
  </component>
  <input type="checkbox" />
</template>
