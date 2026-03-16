export function readE2eEnv(name: string): string | undefined {
  return process.env[`VERTER_E2E_${name}`] ?? process.env[`E2E_${name}`];
}
