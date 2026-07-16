<script lang="ts">
  import type { Snippet } from "svelte";

  type HeaderArgs = [{ title: string; count: number }];

  // Snippet that expects typed args — wrong {@render} payloads below.
  const header: Snippet<HeaderArgs> = (({ title, count }) => {
    void title;
    void count;
  }) as Snippet<HeaderArgs>;
</script>

<!-- Live type errors: wrong argument field types at render site -->
{@render header({ title: "ok", count: "not-a-number" })}
{@render header({ title: 99, count: 1 })}
