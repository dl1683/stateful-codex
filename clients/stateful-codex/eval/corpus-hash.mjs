import { createHash } from "node:crypto";
import { readdir, readFile } from "node:fs/promises";
import path from "node:path";

export async function corpusHash(root) {
  const files = await listFiles(root);
  const hash = createHash("sha256");
  for (const relativePath of files) {
    const content = await readFile(path.join(root, relativePath));
    hash.update(Buffer.from(String(Buffer.byteLength(relativePath)), "utf8"));
    hash.update(Buffer.from([0]));
    hash.update(relativePath);
    hash.update(Buffer.from([0]));
    hash.update(Buffer.from(String(content.length), "utf8"));
    hash.update(Buffer.from([0]));
    hash.update(content);
  }
  return { sha256: hash.digest("hex"), files: files.length };
}

async function listFiles(root, relative = "") {
  const entries = await readdir(path.join(root, relative), { withFileTypes: true });
  const files = [];
  for (const entry of entries) {
    const child = path.join(relative, entry.name);
    if (entry.isDirectory()) files.push(...(await listFiles(root, child)));
    else if (entry.isFile()) files.push(child.split(path.sep).join("/"));
    else throw new Error(`unsupported corpus entry: ${child}`);
  }
  return files.sort();
}
