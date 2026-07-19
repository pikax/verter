// D7 control: the SAME member-access shape as DomEventHandler.svelte's script,
// carried by a plain `.js` file in the same lax jsconfig project. If the
// carrier lane were healthy this file and the `.svelte` script hover must
// behave identically on every route; a null hover on the `.svelte` with a live
// hover here isolates the defect to the `.svelte` carrier lane, not the
// provider.
function handlePointer(domEvent) {
  const horizontal = domEvent.clientX;
  const mouseButton = domEvent.button;
  return `${mouseButton}:${horizontal}`;
}

export { handlePointer };
