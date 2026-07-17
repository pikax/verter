<script>
  let lastPointerId = $state(0);

  /** @param {PointerEvent} e */
  function myClick(e) {
    e.pointerId;
    // @ts-expect-error PointerEvent has no such member; this must not degrade to any
    lastPointerId = e.__verterMissingPointerMember;
  }
</script>

<button onpointerdown={myClick}>Click</button>
<output>{lastPointerId}</output>
