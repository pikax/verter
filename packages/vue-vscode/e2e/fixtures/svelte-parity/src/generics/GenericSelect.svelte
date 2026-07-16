<script lang="ts" generic="T">
  import type { Snippet } from "svelte";

  /**
   * Svelte advanced generic: T from options flows into value, callbacks, and snippets.
   */
  let {
    options,
    value,
    label = "",
    onSelect,
    onChange,
    option,
    selected,
  }: {
    options: T[];
    value: T;
    label?: string;
    onSelect?: (v: T) => void;
    onChange?: (v: T) => void;
    option?: Snippet<[{ item: T; index: number }]>;
    selected?: Snippet<[{ value: T }]>;
  } = $props();

  function pick(v: T) {
    onSelect?.(v);
    onChange?.(v);
  }
</script>

<div>
  {#if label}<span>{label}</span>{/if}
  {#if selected}
    {@render selected({ value })}
  {/if}
  {#each options as opt, i (i)}
    <button type="button" onclick={() => pick(opt)}>
      {#if option}
        {@render option({ item: opt, index: i })}
      {:else}
        {String(opt)}
      {/if}
    </button>
  {/each}
</div>
