type WorkerRequest = { kind: "ping" };
type WorkerReply = { reply: string };

self.addEventListener("message", (event: MessageEvent<WorkerRequest>) => {
  if (event.data.kind === "ping") {
    (self as DedicatedWorkerGlobalScope).postMessage({ reply: "pong" } satisfies WorkerReply);
  }
});
