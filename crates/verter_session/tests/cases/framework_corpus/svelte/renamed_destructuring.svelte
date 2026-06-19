<script lang="ts">
  // A renamed destructuring binding (`size: s`) plus a default. The PROP name
  // stays `size` (from the Props type), not the local alias `s`.
  interface Props { size?: string; count: number }
  let { size: s = 'md', count }: Props = $props();
</script>
<span class={s}>{count}</span>
