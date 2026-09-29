import { cp, link, mkdir, readFile, stat, writeFile } from "node:fs/promises";
import path from "node:path";

export async function prepareIsolatedHome(root, authHome) {
  const codexHome = path.join(root, "codex-home");
  const sqliteHome = path.join(root, "sqlite");
  await mkdir(codexHome, { recursive: true });
  await mkdir(sqliteHome, { recursive: true });
  const auth = path.join(authHome, "auth.json");
  await link(auth, path.join(codexHome, "auth.json"));
  for (const name of [".sandbox", ".sandbox-bin", "cap_sid"]) {
    const source = path.join(authHome, name);
    const target = path.join(codexHome, name);
    try { await cp(source, target, { recursive: true, errorOnExist: true }); }
    catch (error) { throw new Error(`unable to seed ${name}: ${error.message}`); }
  }
  return { codexHome, sqliteHome };
}

export function isolatedEnvironment({ codexHome, sqliteHome, extra = {} }) {
  const env = { ...process.env, ...extra, CODEX_HOME: codexHome, CODEX_SQLITE_HOME: sqliteHome };
  delete env.OPENAI_API_KEY;
  delete env.CODEX_API_KEY;
  return env;
}
