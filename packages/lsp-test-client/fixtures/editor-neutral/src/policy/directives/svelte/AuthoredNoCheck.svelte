<script>
  // @ts-nocheck
  /** @param {PointerEvent} event */
  function handlePointer(event) {
    return event.__verterMissingPointerMember;
  }
</script>

<button onpointerdown={handlePointer}>No check</button>
