import { rpc, subscribe } from "./rpc.mjs";
import { renderSetup } from "./setup-view.mjs";

const state = {
  busy: false,
  error: null,
  account: null,
  projects: [],
  threads: [],
  projectId: "new",
  projectName: "",
  rootPath: "",
  threadAction: "create",
  threadId: "",
  mode: "collaborative",
  goal: "",
  maxContinuations: 24,
  maxElapsedSeconds: 14400,
};

const app = document.querySelector("#app");

async function boot() {
  subscribe(handleEvent);
  try {
    const [account, projects] = await Promise.all([
      rpc("account/read", { refreshToken: false }),
      rpc("project/list", {
        limit: 100,
        sortKey: "recencyAt",
        sortDirection: "desc",
      }),
    ]);
    state.account = account.account;
    state.projects = projects.data;
    render();
  } catch (error) {
    state.error = error.message;
    render();
  }
}

function render() {
  app.innerHTML = renderSetup(state);
}

function captureForm() {
  const form = app.querySelector("#setup-form");
  if (!form) return;
  const data = new FormData(form);
  for (const key of [
    "projectId",
    "projectName",
    "rootPath",
    "threadId",
    "goal",
  ]) {
    if (data.has(key)) state[key] = String(data.get(key));
  }
  if (data.has("maxContinuations"))
    state.maxContinuations = Number(data.get("maxContinuations"));
  if (data.has("maxElapsedSeconds"))
    state.maxElapsedSeconds = Number(data.get("maxElapsedSeconds"));
}

app.addEventListener("input", captureForm);
app.addEventListener("change", async (event) => {
  captureForm();
  if (event.target.name === "projectId") await loadThreads();
  render();
});
app.addEventListener("click", (event) => {
  const choiceButton = event.target.closest("[data-choice-group]");
  if (!choiceButton) return;
  captureForm();
  state[choiceButton.dataset.choiceGroup === "mode" ? "mode" : "threadAction"] =
    choiceButton.dataset.choiceValue;
  render();
});
app.addEventListener("submit", async (event) => {
  if (event.target.id !== "setup-form") return;
  event.preventDefault();
  captureForm();
  state.busy = true;
  state.error = null;
  render();
  try {
    await openWorkspace();
  } catch (error) {
    state.error = error.message;
    state.busy = false;
    render();
  }
});

async function loadThreads() {
  state.threads = [];
  state.threadId = "";
  if (state.projectId === "new") return;
  const response = await rpc("thread/list", {
    projectId: state.projectId,
    limit: 100,
    sortKey: "recency_at",
    sortDirection: "desc",
    archived: false,
  });
  state.threads = response.data;
}

async function openWorkspace() {
  let project = state.projects.find(
    (candidate) => candidate.id === state.projectId,
  );
  if (!project) {
    const response = await rpc("project/create", {
      name: state.projectName,
      roots: [{ path: state.rootPath }],
      metadata: {},
      idempotencyKey: crypto.randomUUID(),
    });
    project = response.project;
  }
  const root = project.roots[0].path;
  let thread;
  if (state.threadAction === "create") {
    thread = (
      await rpc("thread/start", {
        cwd: root,
        runtimeWorkspaceRoots: project.roots.map((item) => item.path),
        projectId: project.id,
      })
    ).thread;
  } else if (state.threadAction === "continue") {
    thread = (
      await rpc("thread/resume", {
        threadId: state.threadId,
        excludeTurns: true,
      })
    ).thread;
  } else {
    thread = (
      await rpc("thread/fork", {
        threadId: state.threadId,
        cwd: root,
        runtimeWorkspaceRoots: project.roots.map((item) => item.path),
        excludeTurns: true,
      })
    ).thread;
  }
  sessionStorage.setItem("stateful-project", project.id);
  sessionStorage.setItem("stateful-thread", thread.id);
  sessionStorage.setItem("stateful-mode", state.mode);
  sessionStorage.setItem("stateful-goal", state.goal);
  sessionStorage.setItem("stateful-thread-action", state.threadAction);
  sessionStorage.removeItem("stateful-created-run-id");
  sessionStorage.removeItem("stateful-initial-turn-sent");
  sessionStorage.removeItem("stateful-run-key");
  sessionStorage.setItem(
    "stateful-max-continuations",
    String(state.maxContinuations),
  );
  sessionStorage.setItem(
    "stateful-max-elapsed-seconds",
    String(state.maxElapsedSeconds),
  );
  window.location.assign("/workspace.html");
}

function handleEvent(message) {
  if (message.method === "gateway/error") {
    state.error = message.params.message;
    render();
  }
}

boot();
