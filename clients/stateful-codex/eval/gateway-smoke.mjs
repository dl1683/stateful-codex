// Manual browser smoke, explicitly fixture-backed. Never reads a real Codex account or store.
import { mkdtemp, writeFile } from "node:fs/promises";
import { join } from "node:path";
import { fileURLToPath } from "node:url";
import { createGateway } from "../server.mjs";
import { createGatewayFixture } from "../test/gateway-fixture.mjs";

const root = await mkdtemp(fileURLToPath(new URL("./artifacts/p3bc-project-", import.meta.url)));
await writeFile(join(root, "README.md"), "Disposable gateway fixture project.\n");
const server = createGateway(createGatewayFixture(root));
server.listen(0, "127.0.0.1", () => {
  console.log(`Fixture-backed gateway: http://127.0.0.1:${server.address().port}`);
  console.log(`Disposable project: ${root}`);
});
