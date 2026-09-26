import { reply, rpc, subscribe } from "./rpc.mjs";
import { renderWorkspace } from "./workspace-view.mjs";

const projectId = sessionStorage.getItem("stateful-project");
const threadId = sessionStorage.getItem("stateful-thread");
let selectedMode = sessionStorage.getItem("stateful-mode");
const initialGoal = sessionStorage.getItem("stateful-goal");
const app = document.querySelector("#app");

if (!projectId || !threadId || !selectedMode || !initialGoal) {
  window.location.replace("/");
}

const state = {
  projectId,
  threadId,
  project: null,
  run: null,
  recovery: null,
  status: null,
  hierarchy: [],
  blackboard: [],
  obligations: [],
  steering: [],
  activity: [],
  contextHits: [],
  evidence: null,
  pendingRequests: [],
  liveText: "",
  selectedNodeId: null,
  loading: true,
  busyAction: null,
  error: null,
  notice: null,
};

let refreshTimer;
let refreshPromise;

async function boot() {
  subscribe(handleEvent);
  try {
    await ensureRun();
    await refresh();
  } catch (error) {
    fail(error);
  }
}

async function ensureRun() {
  const [projectResponse, runResponse, status] = await Promise.all([
    rpc("project/read", { projectId }),
    rpc("statefulRun/read", runReadParams()),
    rpc("projectIntelligence/status", { projectId }),
  ]);
  state.project = projectResponse.project;
  state.status = status;
  state.recovery = runResponse.recovery;
  if (runResponse.run) {
    state.run = runResponse.run;
    if (
      state.run.mode !== selectedMode &&
      !["completed", "cancelled", "failed"].includes(state.run.status)
    ) {
      const changed = await rpc("statefulRun/setMode", {
        runId: state.run.id,
        expectedRevision: state.run.revision,
        mode: selectedMode,
      });
      state.run = changed.run;
      state.notice = `Mode changed to ${selectedMode} from your setup choice.`;
    } else if (state.run.mode !== selectedMode) {
      selectedMode = state.run.mode;
      sessionStorage.setItem("stateful-mode", selectedMode);
    }
    if (
      sessionStorage.getItem("stateful-created-run-id") === state.run.id &&
      sessionStorage.getItem("stateful-initial-turn-sent") !== state.run.id
    ) {
      state.busyAction = "Retrying the first turn";
      render();
      await sendInitialTurn();
    }
    return;
  }
  if (!status.initialized) {
    state.busyAction = "Indexing the selected project";
    render();
    await rpc("contextMap/refresh", { projectId });
  }
  const idempotencyKey =
    sessionStorage.getItem("stateful-run-key") ?? crypto.randomUUID();
  sessionStorage.setItem("stateful-run-key", idempotencyKey);
  const started = await rpc("statefulRun/start", {
    projectId,
    threadId,
    goal: initialGoal,
    mode: selectedMode,
    budget: {
      maxContinuations: Number(
        sessionStorage.getItem("stateful-max-continuations") ?? 24,
      ),
      maxElapsedSeconds: Number(
        sessionStorage.getItem("stateful-max-elapsed-seconds") ?? 14400,
      ),
    },
    idempotencyKey,
  });
  state.run = started.run;
  sessionStorage.setItem("stateful-created-run-id", state.run.id);
  state.busyAction = "Starting the first turn";
  render();
  await sendInitialTurn();
}

function refresh() {
  if (!refreshPromise) {
    refreshPromise = refreshWorkspace().finally(() => {
      refreshPromise = null;
    });
  }
  return refreshPromise;
}

async function refreshWorkspace() {
  state.loading = !state.project;
  state.error = null;
  render();
  const [
    project,
    run,
    status,
    hierarchy,
    blackboard,
    obligations,
    steering,
    activity,
  ] = await Promise.all([
    rpc("project/read", { projectId }),
    rpc("statefulRun/read", runReadParams()),
    rpc("projectIntelligence/status", { projectId }),
    readHierarchy(),
    rpc("blackboard/query", { projectId, text: null, limit: 50 }),
    state.run
      ? rpc("obligation/list", {
          runId: state.run.id,
          cursor: null,
          limit: 100,
        })
      : { data: [] },
    state.run
      ? rpc("steering/list", {
          runId: state.run.id,
          cursor: null,
          limit: 100,
        })
      : { data: [] },
    rpc("thread/items/list", {
      threadId,
      cursor: null,
      limit: 100,
      sortDirection: "desc",
    }).catch((error) => {
      if (error.code === -32601) return { data: [] };
      throw error;
    }),
  ]);
  state.project = project.project;
  state.run = run.run;
  state.recovery = run.recovery;
  state.status = status;
  state.hierarchy = hierarchy;
  state.blackboard = blackboard.data;
  state.obligations = obligations.data;
  state.steering = steering.data;
  state.activity = activity.data.reverse();
  state.loading = false;
  state.busyAction = null;
  render();
}

function runReadParams() {
  const runId =
    state.run?.id ?? sessionStorage.getItem("stateful-created-run-id");
  return runId ? { runId } : { threadId };
}

async function readHierarchy() {
  const nodes = [];
  let cursor = null;
  do {
    const page = await rpc("projectIntelligence/tree", {
      projectId,
      cursor,
      limit: 500,
    });
    nodes.push(...page.data);
    cursor = page.nextCursor;
  } while (cursor);
  return nodes;
}

function render() {
  app.innerHTML = renderWorkspace(state);
}

function handleEvent(message) {
  if (Object.hasOwn(message, "id") && message.method) {
    state.pendingRequests = [
      ...state.pendingRequests.filter((item) => item.id !== message.id),
      message,
    ];
    render();
    return;
  }
  if (message.method === "gateway/error") {
    state.notice = message.params.message;
    render();
    return;
  }
  if (message.method === "item/agentMessage/delta") {
    state.liveText += message.params.delta ?? "";
    render();
  }
  if (message.method === "turn/started") state.liveText = "";
  if (
    /^(statefulRun|obligation|steering|blackboard|project|thread|turn)\//.test(
      message.method,
    )
  ) {
    clearTimeout(refreshTimer);
    refreshTimer = setTimeout(() => refresh().catch(fail), 180);
  }
}

app.addEventListener("submit", async (event) => {
  event.preventDefault();
  const form = event.target;
  try {
    if (form.id === "steering-form") {
      const input = new FormData(form).get("steering")?.toString().trim();
      if (!input) return;
      await action("Applying steering", () =>
        rpc("steering/submit", {
          runId: state.run.id,
          input,
          affectedObligationIds: state.obligations.at(-1)
            ? [state.obligations.at(-1).id]
            : [],
          idempotencyKey: crypto.randomUUID(),
        }),
      );
    } else if (form.id === "message-form") {
      const input = new FormData(form).get("message")?.toString().trim();
      if (!input) return;
      await action("Sending instruction", () => sendTurn(input));
    } else if (form.id === "mode-form") {
      const mode = new FormData(form).get("mode")?.toString();
      if (!mode || mode === state.run.mode) return;
      const response = await action("Changing workflow mode", () =>
        rpc("statefulRun/setMode", {
          runId: state.run.id,
          expectedRevision: state.run.revision,
          mode,
        }),
      );
      state.run = response.run;
      selectedMode = state.run.mode;
      sessionStorage.setItem("stateful-mode", selectedMode);
      state.notice = `Mode changed to ${mode}.`;
      await refresh();
    } else if (form.dataset.requestId) {
      await answerUserRequest(form);
    } else if (form.id === "context-search") {
      const text = new FormData(form).get("query")?.toString().trim();
      if (!text) return;
      state.contextHits = (
        await action("Searching source map", () =>
          rpc("contextMap/query", { projectId, text, limit: 20 }),
        )
      ).data;
      render();
    }
    form.reset();
  } catch (error) {
    fail(error);
  }
});

app.addEventListener("click", async (event) => {
  const button = event.target.closest("button[data-action]");
  if (!button) return;
  try {
    switch (button.dataset.action) {
      case "refresh":
        await action("Refreshing workspace", refresh);
        break;
      case "refresh-map":
        await action("Refreshing source map", async () => {
          await rpc("contextMap/refresh", { projectId });
          await refresh();
        });
        break;
      case "pause":
      case "resume":
      case "cancel":
        await controlRun(button.dataset.action);
        break;
      case "maintain":
        await action("Starting intelligence maintenance", () =>
          sendTurn(
            "Maintain the project intelligence now. Review the blackboard and context map for stale or unsupported claims, unresolved questions, contradictions, and non-obvious cross-source connections. Verify exact source material before consequential claims, update structured state, and explain the strategic implications in the obligation packet.",
          ),
        );
        break;
      case "evidence":
        const lineRange = button.dataset.firstLine
          ? {
              start: Number(button.dataset.firstLine),
              end: Number(button.dataset.lastLine),
            }
          : null;
        state.evidence = await action("Verifying exact evidence", () =>
          rpc("evidence/read", {
            projectId,
            contextMapEntryId: button.dataset.entryId,
            lineRange,
            maxBytes: 32768,
          }),
        );
        render();
        break;
      case "confirm-knowledge":
        await action("Confirming project understanding", () =>
          rpc("blackboard/confirm", {
            projectId,
            entryId: button.dataset.entryId,
            expectedRevision: Number(button.dataset.revision),
          }),
        );
        await refresh();
        break;
      case "node":
        state.selectedNodeId =
          state.selectedNodeId === button.dataset.nodeId
            ? null
            : button.dataset.nodeId;
        render();
        break;
      case "approve":
      case "decline":
        await answerApproval(button.dataset.requestId, button.dataset.action);
        break;
    }
  } catch (error) {
    fail(error);
  }
});

async function action(label, operation) {
  state.busyAction = label;
  state.error = null;
  render();
  try {
    return await operation();
  } finally {
    state.busyAction = null;
    render();
  }
}

async function sendTurn(text) {
  return rpc("turn/start", {
    threadId,
    input: [{ type: "text", text, text_elements: [] }],
  });
}

async function sendInitialTurn() {
  await sendTurn(initialGoal);
  sessionStorage.setItem("stateful-initial-turn-sent", state.run.id);
}

async function controlRun(control) {
  await action(
    `${control[0].toUpperCase()}${control.slice(1)} run`,
    async () => {
      const response = await rpc(`statefulRun/${control}`, {
        runId: state.run.id,
        expectedRevision: state.run.revision,
      });
      state.run = response.run;
      await refresh();
    },
  );
}

async function answerApproval(requestId, actionName) {
  const request = state.pendingRequests.find(
    (item) => String(item.id) === requestId,
  );
  if (!request) return;
  await reply({
    id: request.id,
    result: { decision: actionName === "approve" ? "accept" : "decline" },
  });
  state.pendingRequests = state.pendingRequests.filter(
    (item) => item.id !== request.id,
  );
  render();
}

async function answerUserRequest(form) {
  const request = state.pendingRequests.find(
    (item) => String(item.id) === form.dataset.requestId,
  );
  if (!request) return;
  const data = new FormData(form);
  const answers = Object.fromEntries(
    request.params.questions.map((question) => [
      question.id,
      { answers: [data.get(question.id)?.toString() ?? ""] },
    ]),
  );
  await reply({ id: request.id, result: { answers } });
  state.pendingRequests = state.pendingRequests.filter(
    (item) => item.id !== request.id,
  );
  render();
}

function fail(error) {
  state.loading = false;
  state.busyAction = null;
  state.error = error.message;
  render();
}

boot();
