/**
 * Pure readers over the E2E log the language server appends to.
 *
 * The server's handler guard writes one `HANDLER_ENTER <method>` line when a
 * request starts and one `HANDLER_EXIT <method>` line when it finishes.
 * Pairing the two for `did_open` since a log mark says how many document opens
 * the server is still working through. A fresh server epoch has to drain that
 * queue before any per-document wait can start a fair clock: on 2026-09-22 the
 * client replayed 62 `did_open`s to a just-restarted server, each took five to
 * seven seconds behind the others, and a case's own open sat in that queue for
 * the whole 12s it was allowed.
 */
const DID_OPEN_HANDLER = /HANDLER_(ENTER|EXIT) did_open\b/g;

/**
 * Number of `did_open` requests the server has entered but not yet exited,
 * counting only lines written after `floor` (a byte offset from `logMark`).
 *
 * An exit whose enter precedes the floor belongs to the server that was shut
 * down for the restart, not the one being measured, so it never drives the
 * count below zero.
 */
export function didOpenHandlersInFlight(log: string, floor = 0): number {
  const window = floor > 0 ? log.slice(floor) : log;
  let inFlight = 0;
  for (const match of window.matchAll(DID_OPEN_HANDLER)) {
    inFlight = match[1] === "ENTER" ? inFlight + 1 : Math.max(0, inFlight - 1);
  }
  return inFlight;
}
