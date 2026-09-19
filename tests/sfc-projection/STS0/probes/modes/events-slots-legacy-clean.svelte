<!--
  STS0 mode-paired compiler control: the legacy on:click event directive and
  the legacy <slot> outlet in a runes:false component. The pinned
  svelte@5.56.10 compiler accepts both with zero warnings in legacy mode;
  the runes-mode twin (events-slots-runes-deprecated.svelte) is accepted
  with deprecation warnings, so both rows stay semantics "both" — legal in
  each mode, never conflated with unsupported constructs.
-->
<script>
  let clicks = 0;
</script>

<button on:click={() => (clicks += 1)}>clicks: {clicks}</button>
<slot name="outlet">fallback</slot>
