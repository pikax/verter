<script lang="ts">
  // A Svelte child component. Its default export is the component value, so a
  // default import of this file binds the component — the carrier-generic
  // default-export semantics every framework carrier shares.
  export let label: string = "child";
  let svelteChildCount = 0;
  function bumpSvelteChild() {
    svelteChildCount += 1;
  }
</script>

<button on:click={bumpSvelteChild}>{label}: {svelteChildCount}</button>
