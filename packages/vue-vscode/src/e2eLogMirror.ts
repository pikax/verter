type LogMethod = (message: string, ...args: unknown[]) => void;

export interface MirrorableLogChannel {
  append(value: string): void;
  appendLine(value: string): void;
  info: LogMethod;
  warn: LogMethod;
  error: LogMethod;
  debug: LogMethod;
  trace: LogMethod;
}

/**
 * Mirror both extension logging and language-client raw output into the E2E
 * evidence sink while preserving the original VS Code output channel calls.
 */
export function installE2eLogMirror(
  channel: MirrorableLogChannel,
  write: (text: string) => void,
): void {
  const originalAppend = channel.append.bind(channel);
  channel.append = (value: string) => {
    write(value);
    originalAppend(value);
  };

  const originalAppendLine = channel.appendLine.bind(channel);
  channel.appendLine = (value: string) => {
    write(`${value}\n`);
    originalAppendLine(value);
  };

  for (const [method, level] of [
    ["info", "INFO"],
    ["warn", "WARN"],
    ["error", "ERROR"],
    ["debug", "DEBUG"],
    ["trace", "TRACE"],
  ] as const) {
    const original = channel[method].bind(channel);
    channel[method] = (message: string, ...args: unknown[]) => {
      write(`[${level}] ${message}${args.length ? ` ${args.map(String).join(" ")}` : ""}\n`);
      original(message, ...args);
    };
  }
}
