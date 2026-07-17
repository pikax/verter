<script lang="ts">
  let invocationCount = $state(0);

  function handlePointer(domEvent) {
    invocationCount += 1;
    const horizontal = domEvent.clientX;
    const mouseButton = domEvent.button;
    const disabled = domEvent.currentTarget.disabled;
    return `${mouseButton}:${horizontal}:${disabled}`;
  }
</script>

<button data-count={invocationCount} onclick={handlePointer}>inspect pointer</button>
