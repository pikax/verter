// Deterministic journey fixture: prints its argv joined by pipes.
process.stdout.write(`argv:${process.argv.slice(2).join("|")}\n`);
