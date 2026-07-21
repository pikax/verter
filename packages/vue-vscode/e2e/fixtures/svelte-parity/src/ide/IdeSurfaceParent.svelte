<script lang="ts">
  import IdeSurfaceChild from "./IdeSurfaceChild.svelte";
  import { fade } from "svelte/transition";

  let label = $state("ide-label");
  let count = $state(2);

  function onPick(value: string): void {
    void value;
  }

  function onChange(next: number): void {
    void next;
  }

  function highlight(node: HTMLElement, params: { color: string }) {
    node.dataset.highlight = params.color;
    return {
      destroy() {},
    };
  }

  interface IdePerson {
    name: string;
    age: number;
  }

  let person: IdePerson | null = $state({ name: "Ada", age: 36 });
</script>

<div>
  <!-- PROP_ATTR_SITE -->
  <IdeSurfaceChild {label} {count} {onPick} {onChange}>
    {#snippet header({ title, count: slotCount })}
      <span>{title}:{slotCount}</span>
    {/snippet}
    {#snippet children({ body })}
      <p>{body}</p>
    {/snippet}
  </IdeSurfaceChild>

  <!-- second usage for stable anchors -->
  <IdeSurfaceChild {label} {count} {onPick} />

  <!-- NARROW_SITE -->
  {#if person}
    <span>{person.name}</span>
  {/if}

  <!-- CUSTOM_DIRECTIVE_SITE -->
  <p use:highlight={{ color: "red" }}>action</p>
  <em transition:fade>transition</em>

  <!-- PROP_COMPLETE_SITE -->
  <!-- SNIPPET_NAME_COMPLETE_SITE -->
  <!-- DIRECTIVE_COMPLETE_SITE -->
  <!-- EVENT_ATTR_COMPLETE_SITE -->
</div>
