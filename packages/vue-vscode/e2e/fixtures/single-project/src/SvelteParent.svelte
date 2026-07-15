<script lang="ts">
  // A Svelte parent that imports the Svelte child by its default export.
  import SvelteChild from "./SvelteChild.svelte";
  let svelteParentTitle = "parent";
</script>

<main>
  <h1>{svelteParentTitle}</h1>
  <SvelteChild label="from-parent" />
</main>
