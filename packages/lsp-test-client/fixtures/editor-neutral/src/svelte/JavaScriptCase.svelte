<script>
  // @ts-check
  /** @type {{ label: string }} */
  let { label } = $props();
  let svelteJsLocal = $state(1);

  /** @returns {void} */
  export function focus() {}
</script>

<button title={label}>{svelteJsLocal.toFixed(0)}</button>
