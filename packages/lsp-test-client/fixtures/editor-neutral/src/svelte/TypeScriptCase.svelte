<script lang="ts">
  interface Props {
    label: string;
  }

  let { label }: Props = $props();
  let svelteTsLocal = $state(1);

  export function focus(): void {}
</script>

<button title={label}>{svelteTsLocal.toFixed(0)}</button>
