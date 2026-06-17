// Curated semantic oracle - event handler argument typing.
//
// The intended Vue semantics of native event handler arguments: a `@click` handler's
// parameter is the DOM `MouseEvent` and a `@keydown` handler's is `KeyboardEvent`.
// This is the case the emitted-TSX artifact-parity spine most easily lowers into a
// self-consistent but wrong type (a bare `Event`), so the oracle pins the native type.
//
// Each anchor's query target is the LAST identifier on its line.

declare function onClick(handler: (event: MouseEvent) => void): void;
declare function onKeydown(handler: (event: KeyboardEvent) => void): void;

onClick((event) => {
  const clickEvent = event; // @dx-anchor click.event
  void clickEvent;
});

onKeydown((event) => {
  const keyEvent = event; // @dx-anchor keydown.event
  void keyEvent;
});
