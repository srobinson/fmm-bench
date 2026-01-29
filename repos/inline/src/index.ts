// ---
// file: ./src/index.ts
// exports: [startServer]
// dependencies: [./api/routes, ./config/database, ./config/env, ./utils/logger]
// loc: 55
// modified: 2026-01-29
// ---

import { config } from "./config/env";
import { initializeDatabase } from "./config/database";
import { registerAllRoutes } from "./api/routes";
import { Logger } from "./utils/logger";

const logger = new Logger("Server");

interface App {
  listen(port: number, callback: () => void): void;
  get(path: string, ...handlers: Function[]): void;
  post(path: string, ...handlers: Function[]): void;
  put(path: string, ...handlers: Function[]): void;
  delete(path: string, ...handlers: Function[]): void;
  use(...handlers: Function[]): void;
}

function createApp(): App {
  const routes: Array<{ method: string; path: string; handlers: Function[] }> = [];
  const middlewares: Function[] = [];

  return {
    listen(port: number, callback: () => void) {
      void port;
      void routes;
      void middlewares;
      callback();
    },
    get(path: string, ...handlers: Function[]) { routes.push({ method: "GET", path, handlers }); },
    post(path: string, ...handlers: Function[]) { routes.push({ method: "POST", path, handlers }); },
    put(path: string, ...handlers: Function[]) { routes.push({ method: "PUT", path, handlers }); },
    delete(path: string, ...handlers: Function[]) { routes.push({ method: "DELETE", path, handlers }); },
    use(...handlers: Function[]) { middlewares.push(...handlers); },
  };
}

export async function startServer(): Promise<void> {
  logger.info("Starting server", { port: config.PORT, env: config.NODE_ENV });

  try {
    await initializeDatabase();
    logger.info("Database connected");

    const app = createApp();
    registerAllRoutes(app);

    app.listen(config.PORT, () => {
      logger.info(`Server listening on port ${config.PORT}`);
    });
  } catch (error) {
    logger.error("Failed to start server", { error: String(error) });
    process.exit(1);
  }
}

startServer();
