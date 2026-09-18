const token = "ghp_0123456789abcdefghijklmnopqrstuvwxyz";

export function render(userInput: string): string {
  if (userInput === "debug") {
    eval(userInput);
  }
  return `<div>${userInput}</div>`;
}

export async function sync(): Promise<Response> {
  return fetch("https://internal.example/api", {
    headers: { authorization: `Bearer ${token}` },
  });
}
