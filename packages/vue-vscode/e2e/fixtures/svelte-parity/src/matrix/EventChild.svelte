<script lang="ts">
  let {
    label,
    onPick,
  }: {
    label: string;
    onPick?: (v: string) => void;
  } = $props();
</script>

<button type="button" onclick={() => onPick?.(label)}>{label}</button>
