<script>
  let invocationCount = $state(0);

  /** @param {MouseEvent} domEvent */
  function handlePointer(domEvent) {
    invocationCount += 1;
    const horizontal = domEvent.clientX;
    const mouseButton = domEvent.button;
    return `${mouseButton}:${horizontal}`;
  }
</script>

<button data-count={invocationCount} onclick={handlePointer}>
  authored JSDoc mouse event
</button>
