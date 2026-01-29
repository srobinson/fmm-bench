// ---
// file: ./src/auth/permissions.ts
// exports: [RolePermissions, hasPermission, isAdmin, requirePermission]
// dependencies: [../types]
// loc: 47
// modified: 2026-01-29
// ---

import { UserRole } from "../types";

export enum Permission {
  ReadOwnProfile = "read:own_profile",
  UpdateOwnProfile = "update:own_profile",
  DeleteOwnAccount = "delete:own_account",
  ReadAnyProfile = "read:any_profile",
  UpdateAnyProfile = "update:any_profile",
  DeleteAnyAccount = "delete:any_account",
  ListUsers = "list:users",
  ManageRoles = "manage:roles",
  ViewAuditLog = "view:audit_log",
}

export const RolePermissions: ReadonlyMap<UserRole, Permission[]> = new Map([
  [UserRole.Guest, [Permission.ReadOwnProfile]],
  [UserRole.User, [
    Permission.ReadOwnProfile,
    Permission.UpdateOwnProfile,
    Permission.DeleteOwnAccount,
  ]],
  [UserRole.Moderator, [
    Permission.ReadOwnProfile,
    Permission.UpdateOwnProfile,
    Permission.DeleteOwnAccount,
    Permission.ReadAnyProfile,
    Permission.ListUsers,
    Permission.ViewAuditLog,
  ]],
  [UserRole.Admin, Object.values(Permission)],
]);

export function hasPermission(role: UserRole, permission: Permission): boolean {
  const permissions = RolePermissions.get(role);
  if (!permissions) return false;
  return permissions.includes(permission);
}

export function requirePermission(role: UserRole, permission: Permission): void {
  if (!hasPermission(role, permission)) {
    throw new Error(`Insufficient permissions: ${permission} required, role ${role} does not have it`);
  }
}

export function isAdmin(role: UserRole): boolean {
  return role === UserRole.Admin;
}
