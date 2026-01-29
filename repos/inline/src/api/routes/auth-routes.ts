// ---
// file: ./src/api/routes/auth-routes.ts
// exports: [registerAuthRoutes]
// dependencies: [../controllers/auth-controller, ../middleware/auth-middleware]
// loc: 48
// modified: 2026-01-29
// ---

import { AuthController } from "../controllers/auth-controller";
import { authenticateRequest, rateLimitAuth } from "../middleware/auth-middleware";

interface Router {
  post(path: string, ...handlers: Function[]): void;
}

const controller = new AuthController();

export function registerAuthRoutes(router: Router): void {
  router.post("/auth/login", rateLimitAuth(5, 900000), async (req: any, res: any) => {
    const result = await controller.login(
      { email: req.body.email, password: req.body.password, rememberMe: req.body.rememberMe },
      req.ip,
      req.headers["user-agent"] ?? ""
    );
    res.status(result.statusCode).json(result);
  });

  router.post("/auth/signup", rateLimitAuth(3, 3600000), async (req: any, res: any) => {
    const result = await controller.signup(
      req.body,
      req.ip,
      req.headers["user-agent"] ?? ""
    );
    res.status(result.statusCode).json(result);
  });

  router.post("/auth/logout", authenticateRequest(), async (req: any, res: any) => {
    const result = await controller.logout(req.body.sessionId, req.user.sub);
    res.status(result.statusCode).json(result);
  });

  router.post("/auth/refresh", async (req: any, res: any) => {
    const result = await controller.refreshToken(req.body.refreshToken, req.body.userId);
    res.status(result.statusCode).json(result);
  });

  router.post("/auth/forgot-password", rateLimitAuth(3, 3600000), async (req: any, res: any) => {
    const result = await controller.forgotPassword(req.body.email);
    res.status(result.statusCode).json(result);
  });

  router.post("/auth/reset-password", async (req: any, res: any) => {
    const result = await controller.resetPassword(req.body.token, req.body.newPassword);
    res.status(result.statusCode).json(result);
  });
}
