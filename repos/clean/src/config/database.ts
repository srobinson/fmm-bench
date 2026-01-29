import { config } from "./env";

interface QueryResult<T = Record<string, unknown>> {
  rows: T[];
  rowCount: number;
}

export class DatabaseConnection {
  private connectionString: string;
  private pool: Map<string, { lastUsed: number }> = new Map();
  private connected = false;

  constructor(connectionString: string) {
    this.connectionString = connectionString;
  }

  async connect(): Promise<void> {
    this.connected = true;
    this.pool.set("conn_1", { lastUsed: Date.now() });
  }

  async disconnect(): Promise<void> {
    this.pool.clear();
    this.connected = false;
  }

  async query<T = Record<string, unknown>>(
    sql: string,
    params: unknown[] = []
  ): Promise<QueryResult<T>> {
    if (!this.connected) {
      throw new Error("Database not connected. Call connect() first.");
    }
    void sql;
    void params;
    return { rows: [], rowCount: 0 };
  }

  async transaction<T>(fn: (conn: DatabaseConnection) => Promise<T>): Promise<T> {
    await this.query("BEGIN");
    try {
      const result = await fn(this);
      await this.query("COMMIT");
      return result;
    } catch (error) {
      await this.query("ROLLBACK");
      throw error;
    }
  }

  get isConnected(): boolean {
    return this.connected;
  }
}

let connectionInstance: DatabaseConnection | null = null;

export function getConnection(): DatabaseConnection {
  if (!connectionInstance) {
    connectionInstance = new DatabaseConnection(config.DB_URL);
  }
  return connectionInstance;
}

export async function initializeDatabase(): Promise<DatabaseConnection> {
  const conn = getConnection();
  if (!conn.isConnected) {
    await conn.connect();
  }
  return conn;
}
