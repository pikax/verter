<!--
  STS0 mode-paired compiler control: the legacy on:click event directive and
  the legacy <slot> outlet inside a runes:true component. The pinned
  svelte@5.56.10 compiler ACCEPTS both — emitting the deprecation warnings
  event_directive_deprecated and slot_element_deprecated, not errors.
  Deprecation is thereby distinguished from unsupported-mode rejection
  (compare legacy-only-export-let.svelte), keeping both rows semantics
  "both" with their deprecation recorded rather than excluded.
-->
<script>
  let clicks = $state(0);
</script>

<button on:click={() => (clicks += 1)}>clicks: {clicks}</button>
<slot name="outlet">fallback</slot>
