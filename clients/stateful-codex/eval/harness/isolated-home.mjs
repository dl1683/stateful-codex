import { link, mkdir, readdir } from "node:fs/promises";
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
    try {
      if (name === "cap_sid") await link(source, target);
      else await seedRuntimeTree(source, target, name === ".sandbox");
    }
    catch (error) { throw new Error(`unable to seed ${name}: ${error.message}`); }
  }
  return { codexHome, sqliteHome };
}

async function seedRuntimeTree(source, target, skipLogs) {
  await mkdir(target, { recursive: true });
  for (const entry of await readdir(source, { withFileTypes: true })) {
    if (skipLogs && entry.name.endsWith(".log")) continue;
    const sourceEntry = path.join(source, entry.name);
    const targetEntry = path.join(target, entry.name);
    if (entry.isDirectory()) await seedRuntimeTree(sourceEntry, targetEntry, skipLogs);
    else if (entry.isFile()) await link(sourceEntry, targetEntry);
  }
}

export function isolatedEnvironment({ codexHome, sqliteHome, extra = {} }) {
  const env = { ...process.env, ...extra, CODEX_HOME: codexHome, CODEX_SQLITE_HOME: sqliteHome };
  delete env.OPENAI_API_KEY;
  delete env.CODEX_API_KEY;
  return env;
}
