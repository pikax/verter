// Deterministic journey fixture: sleeps long enough to be aborted or timed out.
setTimeout(() => {
  process.stdout.write("late\n");
}, 30_000);
