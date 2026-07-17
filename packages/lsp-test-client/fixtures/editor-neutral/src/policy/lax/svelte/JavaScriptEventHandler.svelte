<script>
  let lastPointerId = $state(0);

  /** @param {PointerEvent} e */
  function myClick(e) {
    e.pointerId;
    // @ts-expect-error authored PointerEvent has no such member; lax policy suppresses diagnostics
    lastPointerId = e.__verterMissingPointerMember;
  }
</script>

<button onpointerdown={myClick}>Click</button>
<output>{lastPointerId}</output>
