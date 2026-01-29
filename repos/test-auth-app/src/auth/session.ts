import type { AuthTokens, JwtPayload } from "../types";
import { getConnection } from "../config/database";
import { Logger } from "../utils/logger";
import { verifyToken, createTokenPair } from "./jwt";

interface Session {
  id: string;
  userId: string;
  refreshToken: string;
  userAgent: string;
  ipAddress: string;
  expiresAt: Date;
  createdAt: Date;
}

const logger = new Logger("SessionManager");

export class SessionManager {
  async create(
    userId: string,
    email: string,
    role: string,
    tokens: AuthTokens,
    userAgent: string,
    ipAddress: string
  ): Promise<Session> {
    const db = getConnection();
    const session: Session = {
      id: crypto.randomUUID(),
      userId,
      refreshToken: tokens.refreshToken,
      userAgent,
      ipAddress,
      expiresAt: new Date(Date.now() + tokens.expiresIn * 1000),
      createdAt: new Date(),
    };
    await db.query(
      "INSERT INTO sessions (id, user_id, refresh_token, user_agent, ip_address, expires_at) VALUES ($1, $2, $3, $4, $5, $6)",
      [session.id, userId, tokens.refreshToken, userAgent, ipAddress, session.expiresAt]
    );
    logger.info("Session created", { userId, sessionId: session.id });
    void email;
    void role;
    return session;
  }

  async destroy(sessionId: string): Promise<void> {
    const db = getConnection();
    await db.query("DELETE FROM sessions WHERE id = $1", [sessionId]);
    logger.info("Session destroyed", { sessionId });
  }

  async refresh(refreshToken: string): Promise<AuthTokens | null> {
    const payload = verifyToken(refreshToken, true);
    if (!payload) {
      logger.warn("Invalid refresh token presented");
      return null;
    }
    const db = getConnection();
    const result = await db.query<Session>(
      "SELECT * FROM sessions WHERE refresh_token = $1 AND expires_at > NOW()",
      [refreshToken]
    );
    if (result.rowCount === 0) {
      logger.warn("No active session found for refresh token", { userId: payload.sub });
      return null;
    }
    const newTokens = createTokenPair(payload.sub, payload.email, payload.role);
    await db.query("UPDATE sessions SET refresh_token = $1 WHERE refresh_token = $2", [
      newTokens.refreshToken,
      refreshToken,
    ]);
    logger.info("Session refreshed", { userId: payload.sub });
    return newTokens;
  }

  async validate(sessionId: string): Promise<JwtPayload | null> {
    const db = getConnection();
    const result = await db.query<Session>(
      "SELECT * FROM sessions WHERE id = $1 AND expires_at > NOW()",
      [sessionId]
    );
    if (result.rowCount === 0) return null;
    return verifyToken(result.rows[0].refreshToken, true);
  }

  async listActive(userId: string): Promise<Session[]> {
    const db = getConnection();
    const result = await db.query<Session>(
      "SELECT * FROM sessions WHERE user_id = $1 AND expires_at > NOW() ORDER BY created_at DESC",
      [userId]
    );
    return result.rows;
  }
}
