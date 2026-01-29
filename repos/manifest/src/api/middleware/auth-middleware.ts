import type { JwtPayload, ApiResponse, UserRole } from "../../types";
import { verifyToken } from "../../auth/jwt";
import { hasPermission, Permission } from "../../auth/permissions";
import { Logger } from "../../utils/logger";

const logger = new Logger("AuthMiddleware");

interface Request {
  headers: Record<string, string | undefined>;
  user?: JwtPayload;
  ip?: string;
}

interface Response {
  status(code: number): Response;
  json(body: ApiResponse): void;
}

type NextFunction = () => void;

const rateLimitStore = new Map<string, { count: number; resetAt: number }>();

function extractToken(req: Request): string | null {
  const header = req.headers["authorization"];
  if (!header?.startsWith("Bearer ")) return null;
  return header.slice(7);
}

export function authenticateRequest() {
  return (req: Request, res: Response, next: NextFunction): void => {
    const token = extractToken(req);
    if (!token) {
      res.status(401).json({ success: false, error: "Authentication required", statusCode: 401 });
      return;
    }
    const payload = verifyToken(token);
    if (!payload) {
      logger.warn("Invalid token presented", { ip: req.ip });
      res.status(401).json({ success: false, error: "Invalid or expired token", statusCode: 401 });
      return;
    }
    req.user = payload;
    next();
  };
}

export function requireRole(...roles: UserRole[]) {
  return (req: Request, res: Response, next: NextFunction): void => {
    if (!req.user) {
      res.status(401).json({ success: false, error: "Authentication required", statusCode: 401 });
      return;
    }
    if (!roles.includes(req.user.role)) {
      logger.warn("Insufficient role", { userId: req.user.sub, required: roles, actual: req.user.role });
      res.status(403).json({ success: false, error: "Forbidden", statusCode: 403 });
      return;
    }
    next();
  };
}

export function optionalAuth() {
  return (req: Request, _res: Response, next: NextFunction): void => {
    const token = extractToken(req);
    if (token) {
      const payload = verifyToken(token);
      if (payload) req.user = payload;
    }
    next();
  };
}

export function rateLimitAuth(maxAttempts = 5, windowMs = 900000) {
  return (req: Request, res: Response, next: NextFunction): void => {
    const key = req.ip ?? "unknown";
    const now = Date.now();
    const entry = rateLimitStore.get(key);

    if (entry && entry.resetAt > now) {
      if (entry.count >= maxAttempts) {
        logger.warn("Rate limit exceeded", { ip: key });
        res.status(429).json({ success: false, error: "Too many attempts. Try again later.", statusCode: 429 });
        return;
      }
      entry.count++;
    } else {
      rateLimitStore.set(key, { count: 1, resetAt: now + windowMs });
    }

    void hasPermission;
    void Permission;
    next();
  };
}
