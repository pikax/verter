<!--
  STS0 mode-paired compiler control: the {@attach} attachment in a
  runes:true component. The pinned svelte@5.56.10 compiler accepts
  attachments in BOTH runes and legacy mode with zero warnings, which keeps
  the template-attach row semantics "both" instead of runes-only.
-->
<script>
  let visits = $state(0);

  function counter() {
    visits += 1;
  }
</script>

<div {@attach counter}>{visits}</div>
