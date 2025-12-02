<script setup lang="ts">
import { useCssModule } from "vue";

// Access CSS module classes programmatically
const styles = useCssModule();
const altStyles = useCssModule("alt");

const items = ["First", "Second", "Third"];
</script>

<template>
  <div :class="$style.container">
    <h2 :class="$style.title">CSS Modules</h2>

    <!-- Using $style -->
    <div :class="$style.box">Default module box</div>

    <!-- Multiple classes -->
    <div :class="[$style.box, $style.highlighted]">Highlighted box</div>

    <!-- Conditional classes -->
    <div :class="{ [$style.box]: true, [$style.active]: true }">
      Active box
    </div>

    <!-- Using useCssModule() -->
    <div :class="styles.box">Using useCssModule()</div>

    <!-- Named module -->
    <div :class="alt.card">Named module (alt)</div>

    <!-- Combining modules -->
    <div :class="[$style.box, alt.bordered]">Combined modules</div>

    <!-- Dynamic classes -->
    <ul :class="$style.list">
      <li
        v-for="(item, index) in items"
        :key="item"
        :class="{ [$style.listItem]: true, [$style.even]: index % 2 === 0 }"
      >
        {{ item }}
      </li>
    </ul>
  </div>
</template>

<!-- Default CSS module -->
<style module>
.container {
  padding: 20px;
}

.title {
  color: navy;
}

.box {
  padding: 10px;
  margin: 10px 0;
  background: #f0f0f0;
  border-radius: 4px;
}

.highlighted {
  background: yellow;
}

.active {
  border: 2px solid blue;
}

.list {
  list-style: none;
  padding: 0;
}

.listItem {
  padding: 5px;
}

.even {
  background: #e0e0e0;
}
</style>

<!-- Named CSS module -->
<style module="alt">
.card {
  padding: 15px;
  background: lavender;
  border-radius: 8px;
}

.bordered {
  border: 3px dashed purple;
}
</style>
