<script>
  let { label, disabled = false, ontoggle } = $props();

  function onClick() {
    ontoggle?.(!disabled);
  }
</script>

<button {disabled} onclick={onClick}>{label}</button>
