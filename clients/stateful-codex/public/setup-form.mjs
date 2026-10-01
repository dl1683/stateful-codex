import { renderSetup, setupBlocker, updateSetup } from "./setup-view.mjs";

const TEXT_FIELDS = new Set([
  "projectId",
  "projectName",
  "rootPath",
  "threadId",
  "goal",
]);
const NUMBER_FIELDS = new Set(["maxContinuations", "maxElapsedSeconds"]);

// Owns the setup page's state transitions. The form is mounted on the first update and then
// only patched, so a click always lands on the element the user pressed and typed drafts stay.
export function createSetupForm({ root, state, rpc, openWorkspace }) {
  let mounted = false;
  let threadGeneration = 0;

  function update() {
    if (!mounted) {
      root.innerHTML = renderSetup(state);
      mounted = true;
    }
    updateSetup(root, state);
  }

  function capture(control) {
    if (!control || control.disabled || control.closest?.("fieldset[disabled]")) {
      return;
    }
    if (TEXT_FIELDS.has(control.name)) state[control.name] = String(control.value);
    if (NUMBER_FIELDS.has(control.name)) state[control.name] = Number(control.value);
  }

  // A slow thread list for an earlier project selection must never replace the current one.
  async function loadThreads() {
    const generation = ++threadGeneration;
    const projectId = state.projectId;
    state.threads = [];
    state.threadId = "";
    state.threadsLoading = projectId !== "new";
    update();
    if (projectId === "new") return;
    try {
      const response = await rpc("thread/list", {
        projectId,
        limit: 100,
        sortKey: "recency_at",
        sortDirection: "desc",
        archived: false,
      });
      if (generation !== threadGeneration) return;
      state.threads = response.data;
    } catch (error) {
      if (generation !== threadGeneration) return;
      state.error = error.message;
    }
    state.threadsLoading = false;
    update();
  }

  root.addEventListener("input", (event) => capture(event.target));
  root.addEventListener("change", (event) => {
    const control = event.target;
    capture(control);
    if (control.name === "projectId") return loadThreads();
    update();
  });
  root.addEventListener("click", (event) => {
    const button = event.target.closest?.("[data-choice-group]");
    if (!button || !root.contains(button)) return;
    const group = button.dataset.choiceGroup === "mode" ? "mode" : "threadAction";
    state[group] = button.dataset.choiceValue;
    update();
  });
  root.addEventListener("submit", async (event) => {
    const form = event.target;
    if (form.id !== "setup-form") return;
    event.preventDefault();
    if (state.busy) return;
    for (const control of form.querySelectorAll("[name]")) capture(control);
    const blocker = setupBlocker(state);
    if (blocker) {
      state.error = blocker;
      update();
      return;
    }
    state.busy = true;
    state.error = null;
    update();
    try {
      await openWorkspace(state);
    } catch (error) {
      state.error = error.message;
      state.busy = false;
      update();
    }
  });

  return {
    update,
    showError(message) {
      state.error = message;
      update();
    },
  };
}
