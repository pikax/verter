<script lang="ts">
  /**
   * Typed child for Ctrl+Click + completion E2E (props, callbacks, snippets).
   */
  let {
    label,
    count,
    enabled = false,
    onPick,
    onChange,
    header,
    children,
  }: {
    label: string;
    count: number;
    enabled?: boolean;
    onPick?: (value: string) => void;
    onChange?: (next: number) => void;
    header?: import("svelte").Snippet<[{ title: string; count: number }]>;
    children?: import("svelte").Snippet<[{ body: string }]>;
  } = $props();
</script>

<button
  type="button"
  onclick={() => {
    onPick?.(label);
    onChange?.(count);
  }}
>
  {label}:{count}:{enabled}
</button>
{#if header}
  <header>
    {@render header({ title: "hdr", count })}
  </header>
{/if}
{#if children}
  <main>
    {@render children({ body: "main" })}
  </main>
{/if}
