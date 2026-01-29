import { getConnection } from "../../config/database";

export enum AuditAction {
  Login = "LOGIN",
  Logout = "LOGOUT",
  Signup = "SIGNUP",
  PasswordChange = "PASSWORD_CHANGE",
  PasswordReset = "PASSWORD_RESET",
  ProfileUpdate = "PROFILE_UPDATE",
  AccountDeletion = "ACCOUNT_DELETION",
  RoleChange = "ROLE_CHANGE",
  TokenRefresh = "TOKEN_REFRESH",
}

interface AuditLogEntry {
  id: string;
  userId: string;
  action: AuditAction;
  metadata: Record<string, unknown>;
  ipAddress: string;
  userAgent: string;
  createdAt: Date;
}

export class AuditLogModel {
  async create(data: {
    userId: string;
    action: AuditAction;
    metadata?: Record<string, unknown>;
    ipAddress?: string;
    userAgent?: string;
  }): Promise<AuditLogEntry> {
    const db = getConnection();
    const entry: AuditLogEntry = {
      id: crypto.randomUUID(),
      userId: data.userId,
      action: data.action,
      metadata: data.metadata ?? {},
      ipAddress: data.ipAddress ?? "unknown",
      userAgent: data.userAgent ?? "unknown",
      createdAt: new Date(),
    };
    await db.query(
      "INSERT INTO audit_logs (id, user_id, action, metadata, ip_address, user_agent, created_at) VALUES ($1,$2,$3,$4,$5,$6,$7)",
      [entry.id, entry.userId, entry.action, JSON.stringify(entry.metadata), entry.ipAddress, entry.userAgent, entry.createdAt]
    );
    return entry;
  }

  async findByUser(userId: string, limit = 50): Promise<AuditLogEntry[]> {
    const db = getConnection();
    const result = await db.query<AuditLogEntry>(
      "SELECT * FROM audit_logs WHERE user_id = $1 ORDER BY created_at DESC LIMIT $2",
      [userId, limit]
    );
    return result.rows;
  }

  async findByAction(action: AuditAction, limit = 50): Promise<AuditLogEntry[]> {
    const db = getConnection();
    const result = await db.query<AuditLogEntry>(
      "SELECT * FROM audit_logs WHERE action = $1 ORDER BY created_at DESC LIMIT $2",
      [action, limit]
    );
    return result.rows;
  }

  async getRecent(limit = 100): Promise<AuditLogEntry[]> {
    const db = getConnection();
    const result = await db.query<AuditLogEntry>(
      "SELECT * FROM audit_logs ORDER BY created_at DESC LIMIT $1",
      [limit]
    );
    return result.rows;
  }
}
