import { rpc, subscribe } from "./rpc.mjs";
import { createSetupForm } from "./setup-form.mjs";

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

const setup = createSetupForm({
  root: document.querySelector("#app"),
  state,
  rpc,
  openWorkspace,
});

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
  } catch (error) {
    state.error = error.message;
  }
  setup.update();
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
    setup.showError(message.params.message);
  }
}

boot();
