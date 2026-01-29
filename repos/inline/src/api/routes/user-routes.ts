// ---
// file: ./src/api/routes/user-routes.ts
// exports: [registerUserRoutes]
// dependencies: [../../types, ../controllers/user-controller, ../middleware/auth-middleware]
// loc: 50
// modified: 2026-01-29
// ---

import { UserController } from "../controllers/user-controller";
import { authenticateRequest, requireRole } from "../middleware/auth-middleware";
import { UserRole } from "../../types";

interface Router {
  get(path: string, ...handlers: Function[]): void;
  put(path: string, ...handlers: Function[]): void;
  delete(path: string, ...handlers: Function[]): void;
}

const controller = new UserController();

export function registerUserRoutes(router: Router): void {
  router.get("/users/me", authenticateRequest(), async (req: any, res: any) => {
    const result = await controller.getProfile(req.user.sub);
    res.status(result.statusCode).json(result);
  });

  router.put("/users/me", authenticateRequest(), async (req: any, res: any) => {
    const result = await controller.updateProfile(req.user.sub, req.user.role, req.body);
    res.status(result.statusCode).json(result);
  });

  router.put("/users/me/password", authenticateRequest(), async (req: any, res: any) => {
    const result = await controller.changePassword(
      req.user.sub,
      req.user.role,
      req.body.currentPassword,
      req.body.newPassword
    );
    res.status(result.statusCode).json(result);
  });

  router.delete("/users/me", authenticateRequest(), async (req: any, res: any) => {
    const result = await controller.deleteAccount(req.user.sub, req.user.role);
    res.status(result.statusCode).json(result);
  });

  router.get("/users", authenticateRequest(), requireRole(UserRole.Admin, UserRole.Moderator), async (req: any, res: any) => {
    const page = parseInt(req.query.page ?? "1", 10);
    const pageSize = parseInt(req.query.pageSize ?? "20", 10);
    const result = await controller.listUsers(req.user.role, page, pageSize);
    res.status(result.statusCode).json(result);
  });

  router.get("/users/:id", authenticateRequest(), requireRole(UserRole.Admin, UserRole.Moderator), async (req: any, res: any) => {
    const result = await controller.getUserById(req.user.role, req.params.id);
    res.status(result.statusCode).json(result);
  });
}
