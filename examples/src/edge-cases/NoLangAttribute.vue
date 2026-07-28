<script setup>
// `<script setup>` with no `lang` attribute — the default, JavaScript, which is
// what this edge case is named for. It previously carried `lang="ts"` while its
// own comment claimed otherwise, which made the SFC invalid Vue: `compileScript`
// throws on an SFC whose `<script>` and `<script setup>` declare different
// languages. Both blocks now omit `lang`, so the example is what it says it is.
</script>

<script>
// Regular script without lang - treated as JS, agreeing with the setup block.
export default {
  name: "NoLangComponent",
};
</script>

<template>
  <div>No lang attribute</div>
</template>
