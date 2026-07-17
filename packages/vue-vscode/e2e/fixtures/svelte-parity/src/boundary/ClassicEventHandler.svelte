<script>
  // @ts-expect-error legacy JavaScript handlers remain uncontextual
  function handlePointer(domEvent) {
    return domEvent.notADomEventMember;
  }
</script>

<button onclick={handlePointer}>legacy boundary</button>
