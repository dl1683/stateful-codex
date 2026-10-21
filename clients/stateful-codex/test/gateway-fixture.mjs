// Deliberately not a native-backend substitute: the live gateway smoke uses this fixture when
// no paired binaries exist. Its project, threads and memory are disposable and unauthenticated.
import { EventRouter } from "../event-router.mjs";
import { workspaceFixture } from "./workspace-fixture.mjs";

export function createGatewayFixture(projectRoot) {
  const fixture = workspaceFixture();
  const project = { id: "fixture-project", name: "Disposable gateway check", roots: [{ path: projectRoot }] };
  let revision = 1;
  let entries = [
    { entryId: "fixture-rule", section: "userRule", content: "Do not publish this disposable project." },
    { entryId: "fixture-background", section: "background", content: "I am testing the memory controls." },
    { entryId: "fixture-excerpt", section: "knowledge", content: "An excerpt with an unseen tail", contentTruncated: true },
    { entryId: "fixture-note", section: "knowledge", content: "This note is on the next page." },
  ].map((item) => ({ revision: 1, source: "user", replaces: [], contentTruncated: false, ...item }));
  const actions = new Map();
  const calls = [];
  const router = new EventRouter({ replyToServer: () => {} });
  const refuse = (message) => { throw Object.assign(new Error(message), { code: -32602 }); };
  return {
    ready: Promise.resolve(),
    calls,
    reply: (envelope) => router.reply(envelope),
    subscribe: (response, scope) => router.subscribe({
      write: (message) => response.write(`data: ${JSON.stringify(message)}\n\n`),
    }, scope),
    async request(method, params) {
      calls.push({ method, params });
      if (method.startsWith("statefulMemory/")) {
        if (params.expectedProjectId !== project.id || params.threadId !== "fixture-thread") refuse("wrong project; nothing changed");
        if (method === "statefulMemory/read") {
          const [projectId, version, offset] = params.cursor?.split(":") ?? [project.id, String(revision), "0"];
          if (projectId !== project.id || Number(version) !== revision) refuse("project memory changed since this page was read");
          const start = Number(offset);
          return { projectId: project.id, data: entries.slice(start, start + 3),
            nextCursor: start + 3 < entries.length ? `${project.id}:${revision}:${start + 3}` : null };
        }
        if (method === "statefulMemory/add") {
          const fingerprint = JSON.stringify([params.kind, params.content, params.reason]);
          const old = actions.get(params.clientActionId);
          if (old && old.fingerprint !== fingerprint) refuse("this action already added something else");
          if (old) return { item: old.item, outcome: "alreadyDone" };
          const item = { entryId: `fixture-add-${revision}`, revision: 1,
            section: { rule: "userRule", note: "knowledge" }[params.kind] ?? params.kind,
            content: params.reason ? `${params.content} Reason: ${params.reason}` : params.content,
            contentTruncated: false, source: "user", replaces: [] };
          entries = [item, ...entries];
          revision += 1;
          actions.set(params.clientActionId, { fingerprint, item });
          return { item, outcome: "added" };
        }
        const item = entries.find((entry) => entry.entryId === params.entryId);
        if (!item || item.revision !== params.expectedRevision) refuse("stale revision; nothing changed");
        entries = entries.filter((entry) => entry !== item);
        revision += 1;
        if (method === "statefulMemory/forget") return { entryId: item.entryId, revision };
        const successor = { ...item, entryId: `fixture-correct-${revision}`, revision: 1, content: params.content,
          replaces: [{ entryId: item.entryId, content: item.content }] };
        entries = [successor, ...entries];
        return { item: successor };
      }
      switch (method) {
        case "account/read": return { account: { type: "chatgpt" } };
        case "project/list": return { data: [project] };
        case "project/read": return { project };
        case "projectIntelligence/status": return fixture.status;
        case "projectIntelligence/tree": return { data: fixture.hierarchy, nextCursor: null };
        case "statefulRun/read": return { run: null, recovery: null };
        case "statefulMeasurement/summary": return { summary: null };
        case "thread/list": return { data: [{ id: "fixture-thread", name: "Disposable thread" }] };
        case "thread/start":
        case "thread/resume": return { thread: { id: "fixture-thread" } };
        case "turn/start": return {};
        default: return { data: [] };
      }
    },
  };
}
