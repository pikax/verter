<script lang="ts">
  /**
   * Typed child surface for negative typecheck fixtures.
   * Props + callback events are intentionally strict.
   */
  let {
    count,
    title,
    enabled = false,
    onPick,
    onChange,
    onClose,
  }: {
    count: number;
    title: string;
    enabled?: boolean;
    onPick?: (value: string) => void;
    onChange?: (next: number) => void;
    onClose?: () => void;
  } = $props();
</script>

<button
  type="button"
  onclick={() => {
    onPick?.(title);
    onChange?.(count);
    onClose?.();
  }}
>
  {title}:{count}:{enabled}
</button>
