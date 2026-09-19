export {};

declare global {
  const STP7_SERVER_SECRET: string;
}

export const fromServer: string = STP7_SERVER_SECRET;
