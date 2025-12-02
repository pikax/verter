<script setup lang="ts">
// Scoped CSS demonstration
</script>

<template>
  <div class="scoped-container">
    <h2>Scoped CSS</h2>

    <p class="text">This text is scoped</p>

    <!-- Deep selector for child components -->
    <div class="parent">
      <ChildComponent />
    </div>

    <!-- Slotted content styling -->
    <div class="slot-wrapper">
      <slot />
    </div>

    <!-- Global escape hatch -->
    <div class="mixed">
      <span class="local">Local style</span>
      <span class="global-text">Global style</span>
    </div>
  </div>
</template>

<script setup lang="ts">
// Mock child component for demo
const ChildComponent = {
  template: `<div class="child-element">Child content</div>`,
};
</script>

<style scoped>
/* Regular scoped styles */
.scoped-container {
  padding: 20px;
}

.text {
  color: blue;
}

/* Deep selector - affects child components */
.parent :deep(.child-element) {
  color: red;
}

/* Alternative deep syntax */
.parent::v-deep(.child-element) {
  font-weight: bold;
}

/* Slotted selector - affects slot content */
.slot-wrapper :slotted(p) {
  color: green;
}

/* Alternative slotted syntax */
.slot-wrapper::v-slotted(span) {
  font-style: italic;
}

/* Global selector - escapes scoping */
:global(.global-text) {
  color: purple;
  font-size: 20px;
}

/* Alternative global syntax */
::v-global(.another-global) {
  text-decoration: underline;
}

/* Mixing local and global in same rule */
.mixed :global(.external-class) {
  opacity: 0.8;
}
</style>

<!-- Additional scoped block -->
<style scoped>
/* Can have multiple scoped style blocks */
.additional-style {
  margin-top: 10px;
}
</style>

<!-- Unscoped global styles -->
<style>
/* These are NOT scoped */
.truly-global {
  font-family: sans-serif;
}
</style>
