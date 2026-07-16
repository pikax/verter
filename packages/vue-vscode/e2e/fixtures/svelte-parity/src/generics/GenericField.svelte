<script lang="ts" generic="T extends string | number">
  let {
    value,
    format,
    disabled = false,
    onChange,
  }: {
    value: T;
    format: (v: T) => string;
    disabled?: boolean;
    onChange?: (next: T) => void;
  } = $props();
</script>

<div>
  <span>{format(value)}</span>
  <button type="button" {disabled} onclick={() => onChange?.(value)}>touch</button>
</div>
