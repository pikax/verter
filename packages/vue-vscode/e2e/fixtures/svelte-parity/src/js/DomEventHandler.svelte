<script>
  let invocationCount = $state(0);

  // @ts-expect-error checked JavaScript keeps unannotated named-handler parameters as any
  function handlePointer(domEvent) {
    invocationCount += 1;
    const horizontal = domEvent.clientX;
    const mouseButton = domEvent.button;
    return `${mouseButton}:${horizontal}`;
  }
</script>

<button data-count={invocationCount} onclick={handlePointer}>inspect pointer</button>
