// ---
// file: ./src/config/env.ts
// exports: [config]
// loc: 20
// modified: 2026-01-29
// ---

function requireEnv(key: string, fallback?: string): string {
  const value = process.env[key] ?? fallback;
  if (value === undefined) {
    throw new Error(`Missing required environment variable: ${key}`);
  }
  return value;
}

export const config = {
  JWT_SECRET: requireEnv("JWT_SECRET", "dev-secret-change-in-production"),
  JWT_EXPIRY: parseInt(requireEnv("JWT_EXPIRY", "900"), 10),
  REFRESH_EXPIRY: parseInt(requireEnv("REFRESH_EXPIRY", "604800"), 10),
  DB_URL: requireEnv("DB_URL", "postgresql://localhost:5432/auth_app"),
  PORT: parseInt(requireEnv("PORT", "3000"), 10),
  BCRYPT_ROUNDS: parseInt(requireEnv("BCRYPT_ROUNDS", "12"), 10),
  NODE_ENV: requireEnv("NODE_ENV", "development"),
  LOG_LEVEL: requireEnv("LOG_LEVEL", "info"),
} as const;

export type Config = typeof config;
