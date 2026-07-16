<script setup lang="ts">
import OuterWrap from "./OuterWrap.vue";
import NoInherit from "./NoInherit.vue";
import FragmentRoot from "./FragmentRoot.vue";
</script>

<template>
  <div>
    <!-- deep chain: class/style/data-/aria should be accepted via fallthrough -->
    <OuterWrap
      tone="primary"
      class="deep-class"
      style="color: red"
      data-testid="deep-leaf"
      aria-label="deep"
    />
    <!-- inheritAttrs:false — extras accepted without unknown-prop noise -->
    <NoInherit label="ni" data-custom="x" class="ni-class" />
    <!-- fragment — non-declared attr should be flagged -->
    <FragmentRoot msg="hello" data-test="frag" />
  </div>
</template>
