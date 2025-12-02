<!-- Template only component - no script block -->
<template>
  <div class="template-only">
    <h1>Template Only Component</h1>
    <p>This component has no script block</p>
    <slot />
  </div>
</template>

<style scoped>
.template-only {
  padding: 20px;
  background: #f5f5f5;
}
</style>
