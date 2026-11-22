export interface Env {
  DB: D1Database;
  TOOLS_BUCKET: R2Bucket;
  ASSETS: Fetcher;
  /** Team password (Worker secret). */
  ADMIN_PASSWORD?: string;
}
