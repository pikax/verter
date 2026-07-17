<script>
  let lastPointerId = $state(0);

  /** @param {PointerEvent} e */
  function myClick(e) {
    e.pointerId;
    lastPointerId = e.__verterMissingPointerMember;
  }
</script>

<button onpointerdown={myClick}>Click</button>
<output>{lastPointerId}</output>
