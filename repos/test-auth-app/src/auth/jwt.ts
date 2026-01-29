import * as crypto from "crypto";
import { config } from "../config/env";
import type { JwtPayload, AuthTokens, UserRole } from "../types";

function base64url(input: string | Buffer): string {
  const str = typeof input === "string" ? Buffer.from(input) : input;
  return str.toString("base64url");
}

function sign(payload: object, secret: string): string {
  const header = base64url(JSON.stringify({ alg: "HS256", typ: "JWT" }));
  const body = base64url(JSON.stringify(payload));
  const signature = crypto.createHmac("sha256", secret).update(`${header}.${body}`).digest("base64url");
  return `${header}.${body}.${signature}`;
}

export function generateAccessToken(userId: string, email: string, role: UserRole): string {
  const payload: JwtPayload = {
    sub: userId,
    email,
    role,
    iat: Math.floor(Date.now() / 1000),
    exp: Math.floor(Date.now() / 1000) + config.JWT_EXPIRY,
    jti: crypto.randomUUID(),
  };
  return sign(payload, config.JWT_SECRET);
}

export function generateRefreshToken(userId: string, email: string, role: UserRole): string {
  const payload: JwtPayload = {
    sub: userId,
    email,
    role,
    iat: Math.floor(Date.now() / 1000),
    exp: Math.floor(Date.now() / 1000) + config.REFRESH_EXPIRY,
    jti: crypto.randomUUID(),
  };
  return sign(payload, config.JWT_SECRET + "_refresh");
}

export function verifyToken(token: string, isRefresh = false): JwtPayload | null {
  try {
    const parts = token.split(".");
    if (parts.length !== 3) return null;
    const secret = isRefresh ? config.JWT_SECRET + "_refresh" : config.JWT_SECRET;
    const expectedSig = crypto.createHmac("sha256", secret).update(`${parts[0]}.${parts[1]}`).digest("base64url");
    if (expectedSig !== parts[2]) return null;
    const payload: JwtPayload = JSON.parse(Buffer.from(parts[1], "base64url").toString());
    if (payload.exp < Math.floor(Date.now() / 1000)) return null;
    return payload;
  } catch {
    return null;
  }
}

export function decodeToken(token: string): JwtPayload | null {
  try {
    const parts = token.split(".");
    if (parts.length !== 3) return null;
    return JSON.parse(Buffer.from(parts[1], "base64url").toString());
  } catch {
    return null;
  }
}

export function createTokenPair(userId: string, email: string, role: UserRole): AuthTokens {
  return {
    accessToken: generateAccessToken(userId, email, role),
    refreshToken: generateRefreshToken(userId, email, role),
    expiresIn: config.JWT_EXPIRY,
    tokenType: "Bearer",
  };
}
