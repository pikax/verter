<script>
  let invocationCount = $state(0);

  // @ts-expect-error checked JavaScript keeps unannotated named-handler parameters as any
  function handleInvalidPointer(domEvent) {
    invocationCount += 1;
    return domEvent.notADomEventMember;
  }
</script>

<button data-count={invocationCount} onclick={handleInvalidPointer}>
  invalid pointer member
</button>
