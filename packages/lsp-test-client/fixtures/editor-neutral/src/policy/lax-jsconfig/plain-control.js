// D7 discriminating control: the SAME JSDoc-annotated member-access shape as
// the JsConfigEventHandler carriers (`vue/` and `svelte/` siblings), carried
// by a plain `.js` file in the same jsconfig project. The hover A/B isolates
// the carrier lane: only the file kind differs, never the typing mechanism.
/** @param {PointerEvent} e */
function myClick(e) {
  e.pointerId;
  return e.__verterMissingPointerMember;
}

export { myClick };
