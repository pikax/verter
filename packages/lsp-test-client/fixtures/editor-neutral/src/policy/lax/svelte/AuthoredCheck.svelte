<script>
  // @ts-check
  /** @param {PointerEvent} event */
  function handlePointer(event) {
    return event.__verterMissingPointerMember;
  }
</script>

<button onpointerdown={handlePointer}>Check</button>
