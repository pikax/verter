const worker = new Worker(new URL("./worker.ts", import.meta.url), { type: "module" });

worker.addEventListener("message", (event: MessageEvent<WorkerReply>) => {
  console.log(event.data.reply);
});

worker.postMessage({ kind: "ping" } satisfies WorkerRequest);
