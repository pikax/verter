<!--
  STS0 mode-paired compiler control: the {@attach} attachment in a
  runes:false (legacy) component. The pinned svelte@5.56.10 compiler
  accepts attachments in BOTH modes with zero warnings; pairing this with
  attach-runes.svelte keeps template-attach semantics "both".
-->
<script>
  let visits = 0;

  function counter() {
    visits += 1;
  }
</script>

<div {@attach counter}>{visits}</div>
