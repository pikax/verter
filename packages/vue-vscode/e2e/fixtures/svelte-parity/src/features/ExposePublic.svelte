<script lang="ts">
  /**
   * Public export surface vs internal binding (Svelte first-class public-surface).
   * secretInternal must not appear on the public component type for non-test importers.
   */
  let publicCount = $state(0);
  let secretInternal = $state("secret");

  export { publicCount };
</script>

<div data-secret={secretInternal}>{publicCount}</div>
