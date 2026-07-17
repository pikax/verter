<script>
  let invocationCount = $state(0);

  function handlePointer(domEvent) {
    invocationCount += 1;
    return domEvent.notADomEventMember;
  }
</script>

<button data-count={invocationCount} onclick={handlePointer}>
  inspect invalid pointer member
</button>
