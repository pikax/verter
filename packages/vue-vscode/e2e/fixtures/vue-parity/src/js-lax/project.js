// D7 control: the SAME member-access shape as DomEventHandler.vue's script,
// carried by a plain `.js` file in the same lax jsconfig project. If the
// carrier lane were healthy this file and the `.vue` script hover must behave
// identically on every route; a null hover on the `.vue` with a live hover
// here isolates the defect to the `.vue` carrier lane, not the provider.
function handlePointer(domEvent) {
  const horizontal = domEvent.clientX;
  const pointerKind = domEvent.pointerType;
  return `${pointerKind}:${horizontal}`;
}

export { handlePointer };
