<script lang="ts">
  /**
   * Opt-in rest surface (Svelte analogue of deliberate attr acceptance).
   * Not Vue fallthrough — author-declared rest only.
   */
  let {
    label,
    ...rest
  }: {
    label: string;
    [key: string]: unknown;
  } = $props();
</script>

<button type="button" {...rest}>{label}</button>
