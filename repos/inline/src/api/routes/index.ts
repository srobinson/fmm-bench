// ---
// file: ./src/api/routes/index.ts
// exports: [registerAllRoutes]
// dependencies: [../../utils/logger, ./auth-routes, ./user-routes]
// loc: 25
// modified: 2026-01-29
// ---

import { registerAuthRoutes } from "./auth-routes";
import { registerUserRoutes } from "./user-routes";
import { Logger } from "../../utils/logger";

const logger = new Logger("Router");

interface Router {
  get(path: string, ...handlers: Function[]): void;
  post(path: string, ...handlers: Function[]): void;
  put(path: string, ...handlers: Function[]): void;
  delete(path: string, ...handlers: Function[]): void;
}

export function registerAllRoutes(router: Router): void {
  logger.info("Registering application routes");

  registerAuthRoutes(router);
  registerUserRoutes(router);

  router.get("/health", (_req: any, res: any) => {
    res.status(200).json({ status: "ok", timestamp: new Date().toISOString() });
  });

  logger.info("All routes registered successfully");
}
