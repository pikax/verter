<script lang="ts">
  import SvelteChild from "./SvelteChild.svelte";
  let mixedRoot = "svelte-root";
</script>

<main>
  <p>{mixedRoot}</p>
  <SvelteChild label={mixedRoot} />
</main>
