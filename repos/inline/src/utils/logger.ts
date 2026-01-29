// ---
// file: ./src/utils/logger.ts
// exports: [Logger]
// dependencies: [../config/env]
// loc: 54
// modified: 2026-01-29
// ---

import { config } from "../config/env";

type LogLevel = "debug" | "info" | "warn" | "error";

const LOG_PRIORITY: Record<LogLevel, number> = {
  debug: 0,
  info: 1,
  warn: 2,
  error: 3,
};

export class Logger {
  private context: string;
  private minLevel: number;

  constructor(context: string) {
    this.context = context;
    this.minLevel = LOG_PRIORITY[(config.LOG_LEVEL as LogLevel) ?? "info"];
  }

  private format(level: LogLevel, message: string, meta?: Record<string, unknown>): string {
    const timestamp = new Date().toISOString();
    const base = `[${timestamp}] [${level.toUpperCase()}] [${this.context}] ${message}`;
    return meta ? `${base} ${JSON.stringify(meta)}` : base;
  }

  private log(level: LogLevel, message: string, meta?: Record<string, unknown>): void {
    if (LOG_PRIORITY[level] < this.minLevel) return;
    const formatted = this.format(level, message, meta);
    if (level === "error") {
      console.error(formatted);
    } else if (level === "warn") {
      console.warn(formatted);
    } else {
      console.log(formatted);
    }
  }

  debug(message: string, meta?: Record<string, unknown>): void {
    this.log("debug", message, meta);
  }

  info(message: string, meta?: Record<string, unknown>): void {
    this.log("info", message, meta);
  }

  warn(message: string, meta?: Record<string, unknown>): void {
    this.log("warn", message, meta);
  }

  error(message: string, meta?: Record<string, unknown>): void {
    this.log("error", message, meta);
  }
}
