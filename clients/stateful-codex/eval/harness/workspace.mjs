import { cp, mkdir, readdir, readFile, writeFile } from "node:fs/promises";
import { execFile } from "node:child_process";
import { promisify } from "node:util";
import path from "node:path";

const run = promisify(execFile);

export async function prepareWorkspace({ fixtureRoot, workspaceRoot }) {
  const fixture = path.resolve(fixtureRoot);
  const workspace = path.resolve(workspaceRoot);
  if (fixture === workspace) throw new Error("fixture source cannot be the output workspace");
  await assertOutsideRepository(workspace);
  await mkdir(path.dirname(workspace), { recursive: true });
  await cp(fixture, workspace, {
    recursive: true,
    filter: (source) => !source.split(path.sep).includes(".git"),
  });
  await git(workspace, ["init"]);
  await git(workspace, ["config", "user.email", "scbench@example.invalid"]);
  await git(workspace, ["config", "user.name", "scbench fixture"]);
  await git(workspace, ["add", "--all"]);
  await git(workspace, ["commit", "--allow-empty", "-m", "fixture baseline"]);
  const [{ stdout: baseline }] = await Promise.all([git(workspace, ["rev-parse", "HEAD"])]);
  return { workspace, baseline: baseline.trim(), files: await listFiles(workspace) };
}

export async function captureWorkspaceDiff(workspace, output) {
  await git(workspace, ["add", "-N", "--", "."]);
  const [{ stdout: diff }, { stdout: status }] = await Promise.all([
    git(workspace, ["diff", "--binary", "HEAD"]),
    git(workspace, ["status", "--porcelain", "--untracked-files=all"]),
  ]);
  await git(workspace, ["reset", "--", "."]);
  await writeFile(output, diff);
  return { patchBytes: Buffer.byteLength(diff), status: status.trim() ? status.trim().split(/\r?\n/) : [] };
}

export async function listFiles(root, relative = "") {
  const entries = await readdir(path.join(root, relative), { withFileTypes: true });
  const files = [];
  for (const entry of entries) {
    if (entry.name === ".git") continue;
    const child = path.join(relative, entry.name);
    if (entry.isDirectory()) files.push(...(await listFiles(root, child)));
    else if (entry.isFile()) files.push(child.split(path.sep).join("/"));
  }
  return files.sort();
}

async function assertOutsideRepository(target) {
  let current = path.dirname(target);
  while (true) {
    try {
      await readFile(path.join(current, ".git"));
      throw new Error(`workspace parent is a Git repository: ${current}`);
    } catch (error) {
      if (error.code !== "ENOENT" && error.message.startsWith("workspace parent")) throw error;
    }
    const parent = path.dirname(current);
    if (parent === current) return;
    current = parent;
  }
}

async function git(cwd, args) {
  return run("git", ["-C", cwd, ...args], { windowsHide: true, maxBuffer: 16 * 1024 * 1024 });
}
