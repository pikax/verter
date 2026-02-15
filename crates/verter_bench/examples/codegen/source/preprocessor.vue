<template>
  <div class="card">
    <header class="card-header">
      <h2>SCSS Preprocessor Test</h2>
    </header>
    <div class="card-body">
      <p>Testing nested selectors and variables</p>
      <ul class="list">
        <li>Item 1</li>
        <li>Item 2</li>
        <li>Item 3</li>
      </ul>
    </div>
    <footer class="card-footer">
      <button class="btn btn-primary">Primary</button>
      <button class="btn btn-secondary">Secondary</button>
    </footer>
  </div>
</template>

<script setup lang="ts">
// No script logic needed for style test
</script>

<style lang="scss" scoped>
// Variables
$primary-color: #007bff;
$secondary-color: #6c757d;
$border-radius: 8px;
$spacing: 16px;

// Mixins
@mixin button-base {
  padding: 8px 16px;
  border: none;
  border-radius: $border-radius / 2;
  cursor: pointer;
  transition: background-color 0.2s;
}

.card {
  border: 1px solid #ddd;
  border-radius: $border-radius;
  overflow: hidden;

  &-header {
    background: #f8f9fa;
    padding: $spacing;
    border-bottom: 1px solid #ddd;

    h2 {
      margin: 0;
      color: #333;
    }
  }

  &-body {
    padding: $spacing;

    .list {
      list-style: none;
      padding: 0;

      li {
        padding: $spacing / 2;
        border-bottom: 1px solid #eee;

        &:last-child {
          border-bottom: none;
        }

        &:hover {
          background: #f5f5f5;
        }
      }
    }
  }

  &-footer {
    padding: $spacing;
    background: #f8f9fa;
    border-top: 1px solid #ddd;
    display: flex;
    gap: $spacing / 2;
  }
}

.btn {
  @include button-base;

  &-primary {
    background: $primary-color;
    color: white;

    &:hover {
      background: darken($primary-color, 10%);
    }
  }

  &-secondary {
    background: $secondary-color;
    color: white;

    &:hover {
      background: darken($secondary-color, 10%);
    }
  }
}
</style>
