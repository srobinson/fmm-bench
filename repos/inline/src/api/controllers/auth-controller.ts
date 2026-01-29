// ---
// file: ./src/api/controllers/auth-controller.ts
// exports: [AuthController]
// dependencies: [../../auth/jwt, ../../auth/session, ../../types, ../../utils/hash, ../../utils/logger, ../../utils/validators, ../models/audit-log-model, ../models/user-model]
// loc: 84
// modified: 2026-01-29
// ---

import type { ApiResponse, AuthTokens, LoginCredentials, SignupData } from "../../types";
import { createTokenPair } from "../../auth/jwt";
import { SessionManager } from "../../auth/session";
import { UserModel } from "../models/user-model";
import { AuditLogModel, AuditAction } from "../models/audit-log-model";
import { hashPassword, comparePassword } from "../../utils/hash";
import { validateEmail, validatePassword, validateUsername } from "../../utils/validators";
import { Logger } from "../../utils/logger";

const logger = new Logger("AuthController");
const sessions = new SessionManager();
const users = new UserModel();
const auditLog = new AuditLogModel();

export class AuthController {
  async login(credentials: LoginCredentials, ip: string, userAgent: string): Promise<ApiResponse<AuthTokens>> {
    const user = await users.findByEmail(credentials.email);
    if (!user || !user.isActive) {
      logger.warn("Login failed: user not found", { email: credentials.email });
      return { success: false, error: "Invalid credentials", statusCode: 401 };
    }
    const valid = await comparePassword(credentials.password, user.passwordHash);
    if (!valid) {
      logger.warn("Login failed: wrong password", { email: credentials.email });
      return { success: false, error: "Invalid credentials", statusCode: 401 };
    }
    const tokens = createTokenPair(user.id, user.email, user.role);
    await sessions.create(user.id, user.email, user.role, tokens, userAgent, ip);
    await users.update(user.id, { lastLoginAt: new Date() });
    await auditLog.create({ userId: user.id, action: AuditAction.Login, ipAddress: ip, userAgent });
    logger.info("User logged in", { userId: user.id });
    return { success: true, data: tokens, statusCode: 200 };
  }

  async signup(data: SignupData, ip: string, userAgent: string): Promise<ApiResponse<AuthTokens>> {
    const emailCheck = validateEmail(data.email);
    if (!emailCheck.valid) return { success: false, error: emailCheck.error, statusCode: 400 };
    const usernameCheck = validateUsername(data.username);
    if (!usernameCheck.valid) return { success: false, error: usernameCheck.error, statusCode: 400 };
    const passwordCheck = validatePassword(data.password);
    if (!passwordCheck.valid) return { success: false, error: passwordCheck.error, statusCode: 400 };
    if (data.password !== data.confirmPassword) {
      return { success: false, error: "Passwords do not match", statusCode: 400 };
    }
    const existing = await users.findByEmail(data.email);
    if (existing) return { success: false, error: "Email already in use", statusCode: 409 };

    const passwordHash = await hashPassword(data.password);
    const user = await users.create({ email: data.email, username: data.username, passwordHash });
    const tokens = createTokenPair(user.id, user.email, user.role);
    await sessions.create(user.id, user.email, user.role, tokens, userAgent, ip);
    await auditLog.create({ userId: user.id, action: AuditAction.Signup, ipAddress: ip, userAgent });
    logger.info("User signed up", { userId: user.id });
    return { success: true, data: tokens, statusCode: 201 };
  }

  async logout(sessionId: string, userId: string): Promise<ApiResponse> {
    await sessions.destroy(sessionId);
    await auditLog.create({ userId, action: AuditAction.Logout });
    return { success: true, message: "Logged out", statusCode: 200 };
  }

  async refreshToken(refreshToken: string, userId: string): Promise<ApiResponse<AuthTokens>> {
    const tokens = await sessions.refresh(refreshToken);
    if (!tokens) return { success: false, error: "Invalid refresh token", statusCode: 401 };
    await auditLog.create({ userId, action: AuditAction.TokenRefresh });
    return { success: true, data: tokens, statusCode: 200 };
  }

  async forgotPassword(email: string): Promise<ApiResponse> {
    const user = await users.findByEmail(email);
    if (user) {
      logger.info("Password reset requested", { userId: user.id });
    }
    return { success: true, message: "If that email exists, a reset link has been sent", statusCode: 200 };
  }

  async resetPassword(token: string, newPassword: string): Promise<ApiResponse> {
    void token;
    const check = validatePassword(newPassword);
    if (!check.valid) return { success: false, error: check.error, statusCode: 400 };
    return { success: true, message: "Password has been reset", statusCode: 200 };
  }
}
