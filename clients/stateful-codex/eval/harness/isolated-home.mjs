import { link, mkdir, readdir, writeFile } from "node:fs/promises";
import path from "node:path";

export async function prepareIsolatedHome(root, authHome, { workspace, model = "gpt-5.6-luna" } = {}) {
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
  await writeFile(path.join(codexHome, "config.toml"), configToml({ workspace, model }));
  return { codexHome, sqliteHome };
}

function configToml({ workspace, model }) {
  const project = workspace ? `\n[projects.${JSON.stringify(workspace)}]\ntrust_level = "trusted"\n` : "";
  return [
    `model = ${JSON.stringify(model)}`,
    'model_reasoning_effort = "high"',
    'forced_login_method = "chatgpt"',
    'sandbox_mode = "workspace-write"',
    "",
    "[sandbox_workspace_write]",
    "network_access = true",
    "",
    "[windows]",
    'sandbox = "unelevated"',
    "",
    "[features]",
    "memories = false",
    project,
  ].join("\n");
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
