<script lang="ts">
  import IdeSurfaceChild from "./IdeSurfaceChild.svelte";

  let label = $state("ide-label");
  let count = $state(2);

  function onPick(value: string): void {
    void value;
  }

  function onChange(next: number): void {
    void next;
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

  <!-- PROP_COMPLETE_SITE -->
  <!-- SNIPPET_NAME_COMPLETE_SITE -->
  <!-- DIRECTIVE_COMPLETE_SITE -->
  <!-- EVENT_ATTR_COMPLETE_SITE -->
</div>
