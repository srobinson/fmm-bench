import type { User, PaginatedResponse } from "../../types";
import { UserRole } from "../../types";
import { getConnection } from "../../config/database";
import { Logger } from "../../utils/logger";

const logger = new Logger("UserModel");

export class UserModel {
  async findById(id: string): Promise<User | null> {
    const db = getConnection();
    const result = await db.query<User>("SELECT * FROM users WHERE id = $1", [id]);
    return result.rows[0] ?? null;
  }

  async findByEmail(email: string): Promise<User | null> {
    const db = getConnection();
    const result = await db.query<User>(
      "SELECT * FROM users WHERE email = $1",
      [email.toLowerCase()]
    );
    return result.rows[0] ?? null;
  }

  async create(data: {
    email: string;
    username: string;
    passwordHash: string;
  }): Promise<User> {
    const db = getConnection();
    const user: User = {
      id: crypto.randomUUID(),
      email: data.email.toLowerCase(),
      username: data.username,
      passwordHash: data.passwordHash,
      role: UserRole.User,
      isActive: true,
      emailVerified: false,
      lastLoginAt: null,
      createdAt: new Date(),
      updatedAt: new Date(),
    };
    await db.query(
      "INSERT INTO users (id, email, username, password_hash, role, is_active, email_verified, created_at, updated_at) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9)",
      [user.id, user.email, user.username, user.passwordHash, user.role, user.isActive, user.emailVerified, user.createdAt, user.updatedAt]
    );
    logger.info("User created", { userId: user.id, email: user.email });
    return user;
  }

  async update(id: string, fields: Partial<Pick<User, "username" | "email" | "passwordHash" | "role" | "isActive" | "emailVerified" | "lastLoginAt">>): Promise<User | null> {
    const db = getConnection();
    const setClauses: string[] = [];
    const values: unknown[] = [];
    let paramIndex = 1;

    for (const [key, value] of Object.entries(fields)) {
      setClauses.push(`${key} = $${paramIndex++}`);
      values.push(value);
    }
    setClauses.push(`updated_at = $${paramIndex++}`);
    values.push(new Date());
    values.push(id);

    await db.query(
      `UPDATE users SET ${setClauses.join(", ")} WHERE id = $${paramIndex}`,
      values
    );
    return this.findById(id);
  }

  async delete(id: string): Promise<boolean> {
    const db = getConnection();
    const result = await db.query("DELETE FROM users WHERE id = $1", [id]);
    logger.info("User deleted", { userId: id });
    return result.rowCount > 0;
  }

  async list(page = 1, pageSize = 20): Promise<PaginatedResponse<User>> {
    const db = getConnection();
    const offset = (page - 1) * pageSize;
    const countResult = await db.query<{ count: number }>("SELECT COUNT(*) as count FROM users");
    const total = countResult.rows[0]?.count ?? 0;
    const result = await db.query<User>(
      "SELECT * FROM users ORDER BY created_at DESC LIMIT $1 OFFSET $2",
      [pageSize, offset]
    );
    const totalPages = Math.ceil(total / pageSize);
    return {
      items: result.rows,
      total,
      page,
      pageSize,
      totalPages,
      hasNext: page < totalPages,
      hasPrevious: page > 1,
    };
  }
}
