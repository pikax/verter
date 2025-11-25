# Copilot Instructions

## Testing

This project uses **Vitest** for testing. When running tests, always use the `--run` flag to execute tests once and exit (non-watch mode):

```bash
pnpm vitest --run
```

Or for a specific test file:

```bash
pnpm vitest --run path/to/test.spec.ts
```

Without the `--run` flag, Vitest will start in watch mode which is not ideal for CI or one-off test runs.
