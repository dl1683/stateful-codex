import { execFile } from "node:child_process";
import { promisify } from "node:util";
import { copyFile, mkdir, readFile, stat, writeFile } from "node:fs/promises";
import path from "node:path";

import { corpusHash } from "./corpus-hash.mjs";

const DEFAULT_EXCLUDES = new Set([
  ".git",
  ".blackboard",
  ".codex",
  ".next",
  ".pytest_cache",
  ".ruff_cache",
  ".venv",
  "__pycache__",
  "node_modules",
  "target",
]);
const execFileAsync = promisify(execFile);

async function main() {
  const options = parseArgs(process.argv.slice(2));
  await assertMissing(options.output);
  const manifest = JSON.parse(await readFile(options.manifest, "utf8"));
  await mkdir(options.output, { recursive: false });
  const prepared = [];
  for (const project of manifest.projects) {
    const sourceRoot = path.join(
      options.projectsRoot,
      project.source.project,
      project.source.subdir ?? "",
    );
    const projectRoot = path.join(options.output, project.id);
    const baseline = path.join(projectRoot, "baseline");
    const stateful = path.join(projectRoot, "stateful");
    await Promise.all([mkdir(baseline, { recursive: true }), mkdir(stateful, { recursive: true })]);
    await Promise.all([
      copySelection(sourceRoot, baseline, project.source),
      copySelection(sourceRoot, stateful, project.source),
    ]);
    const [baselineHash, statefulHash] = await Promise.all([
      corpusHash(baseline),
      corpusHash(stateful),
    ]);
    if (baselineHash.sha256 !== statefulHash.sha256) {
      throw new Error(`isolated copies differ for ${project.id}`);
    }
    const record = {
      id: project.id,
      sourceRoot,
      baseline,
      stateful,
      corpus: baselineHash,
    };
    prepared.push(record);
    await writeFile(
      path.join(projectRoot, "snapshot.json"),
      `${JSON.stringify(record, null, 2)}\n`,
    );
    console.error(`prepared ${project.id}: ${baselineHash.files} files ${baselineHash.sha256}`);
  }
  await writeFile(
    path.join(options.output, "snapshots.json"),
    `${JSON.stringify({ manifest: options.manifest, projects: prepared }, null, 2)}\n`,
  );
}

async function copySelection(sourceRoot, destination, selection) {
  const excludes = new Set([...DEFAULT_EXCLUDES, ...(selection.excludeNames ?? [])]);
  const files = new Set(selection.rootFiles ?? []);
  for (const root of selection.roots ?? ["."]) {
    const searchRoot = path.join(sourceRoot, root);
    const { stdout } = await execFileAsync("rg", ["--files", searchRoot], {
      maxBuffer: 64 * 1024 * 1024,
    });
    for (const candidate of stdout.split(/\r?\n/).filter(Boolean)) {
      const relative = path.relative(sourceRoot, candidate);
      if (!isExcluded(sourceRoot, candidate, excludes)) files.add(relative);
    }
  }
  for (const relativePath of [...files].sort()) {
    const source = path.join(sourceRoot, relativePath);
    const target = path.join(destination, relativePath);
    await mkdir(path.dirname(target), { recursive: true });
    await copyFile(source, target);
  }
}

function isExcluded(sourceRoot, candidate, excludes) {
  const relative = path.relative(sourceRoot, candidate);
  const segments = relative.split(path.sep);
  return segments.some((segment) =>
    excludes.has(segment) ||
    (segment.startsWith(".env") && segment !== ".env.example"),
  );
}

async function assertMissing(target) {
  try {
    await stat(target);
  } catch (error) {
    if (error.code === "ENOENT") return;
    throw error;
  }
  throw new Error(`output already exists: ${target}`);
}

function parseArgs(args) {
  const options = {};
  for (let index = 0; index < args.length; index += 2) {
    const [argument, value] = args.slice(index, index + 2);
    if (argument === "--manifest") options.manifest = value;
    else if (argument === "--projects-root") options.projectsRoot = value;
    else if (argument === "--output") options.output = value;
    else throw new Error(`unknown argument: ${argument}`);
  }
  if (!options.manifest || !options.projectsRoot || !options.output) {
    throw new Error("usage: --manifest PATH --projects-root PATH --output PATH");
  }
  options.manifest = path.resolve(options.manifest);
  options.projectsRoot = path.resolve(options.projectsRoot);
  options.output = path.resolve(options.output);
  return options;
}

await main();
