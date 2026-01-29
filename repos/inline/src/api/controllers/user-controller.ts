// ---
// file: ./src/api/controllers/user-controller.ts
// exports: [UserController]
// dependencies: [../../auth/permissions, ../../types, ../../utils/hash, ../../utils/validators, ../models/audit-log-model, ../models/user-model]
// loc: 79
// modified: 2026-01-29
// ---

import type { ApiResponse, PaginatedResponse, User } from "../../types";
import { UserModel } from "../models/user-model";
import { AuditLogModel, AuditAction } from "../models/audit-log-model";
import { hashPassword, comparePassword } from "../../utils/hash";
import { validateEmail, validateUsername, validatePassword } from "../../utils/validators";
import { Permission, requirePermission, hasPermission } from "../../auth/permissions";
import { UserRole } from "../../types";

const users = new UserModel();
const auditLog = new AuditLogModel();

type SafeUser = Omit<User, "passwordHash">;

function stripPassword(user: User): SafeUser {
  const { passwordHash: _, ...safe } = user;
  return safe;
}

export class UserController {
  async getProfile(userId: string): Promise<ApiResponse<SafeUser>> {
    const user = await users.findById(userId);
    if (!user) return { success: false, error: "User not found", statusCode: 404 };
    return { success: true, data: stripPassword(user), statusCode: 200 };
  }

  async updateProfile(userId: string, role: UserRole, updates: { email?: string; username?: string }): Promise<ApiResponse<SafeUser>> {
    requirePermission(role, Permission.UpdateOwnProfile);
    if (updates.email) {
      const check = validateEmail(updates.email);
      if (!check.valid) return { success: false, error: check.error, statusCode: 400 };
    }
    if (updates.username) {
      const check = validateUsername(updates.username);
      if (!check.valid) return { success: false, error: check.error, statusCode: 400 };
    }
    const updated = await users.update(userId, updates);
    if (!updated) return { success: false, error: "User not found", statusCode: 404 };
    await auditLog.create({ userId, action: AuditAction.ProfileUpdate, metadata: { fields: Object.keys(updates) } });
    return { success: true, data: stripPassword(updated), statusCode: 200 };
  }

  async changePassword(userId: string, role: UserRole, currentPassword: string, newPassword: string): Promise<ApiResponse> {
    requirePermission(role, Permission.UpdateOwnProfile);
    const user = await users.findById(userId);
    if (!user) return { success: false, error: "User not found", statusCode: 404 };
    const valid = await comparePassword(currentPassword, user.passwordHash);
    if (!valid) return { success: false, error: "Current password is incorrect", statusCode: 401 };
    const check = validatePassword(newPassword);
    if (!check.valid) return { success: false, error: check.error, statusCode: 400 };
    const passwordHash = await hashPassword(newPassword);
    await users.update(userId, { passwordHash });
    await auditLog.create({ userId, action: AuditAction.PasswordChange });
    return { success: true, message: "Password changed", statusCode: 200 };
  }

  async deleteAccount(userId: string, role: UserRole): Promise<ApiResponse> {
    requirePermission(role, Permission.DeleteOwnAccount);
    const deleted = await users.delete(userId);
    if (!deleted) return { success: false, error: "User not found", statusCode: 404 };
    await auditLog.create({ userId, action: AuditAction.AccountDeletion });
    return { success: true, message: "Account deleted", statusCode: 200 };
  }

  async listUsers(role: UserRole, page: number, pageSize: number): Promise<ApiResponse<PaginatedResponse<SafeUser>>> {
    requirePermission(role, Permission.ListUsers);
    const result = await users.list(page, pageSize);
    const safeItems = result.items.map(stripPassword);
    return { success: true, data: { ...result, items: safeItems }, statusCode: 200 };
  }

  async getUserById(requestingRole: UserRole, targetId: string): Promise<ApiResponse<SafeUser>> {
    if (!hasPermission(requestingRole, Permission.ReadAnyProfile)) {
      return { success: false, error: "Forbidden", statusCode: 403 };
    }
    const user = await users.findById(targetId);
    if (!user) return { success: false, error: "User not found", statusCode: 404 };
    return { success: true, data: stripPassword(user), statusCode: 200 };
  }
}
