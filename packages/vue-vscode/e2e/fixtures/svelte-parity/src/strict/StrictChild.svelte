<script lang="ts">
  /**
   * Svelte has no Vue-style multi-hop fallthrough. Declared props only unless
   * the author opts into rest props. Invented props must type-error.
   */
  let {
    label,
  }: {
    label: string;
  } = $props();
</script>

<button type="button">{label}</button>
