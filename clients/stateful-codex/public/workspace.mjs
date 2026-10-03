import { documentTitle } from "./memory-view.mjs";
import { reply, rpc, subscribe } from "./rpc.mjs";
import { findTurnMeasurement, latestAnswerTurn } from "./answer-provenance.mjs";
import { createRefreshGate, needsProjectRefresh } from "./refresh-policy.mjs";
import { applyWorkspaceEvent } from "./workspace-events.mjs";
import { applySnapshot, beginSnapshot, createExecution } from "./execution-state.mjs";
import { createSummaryReader, readExecutionSnapshot, readRecap } from "./status-reads.mjs";
import { createSourceSearch } from "./source-search.mjs";
import { submitSteering } from "./steering-submit.mjs";
import {
  readPendingFollowUp,
  resumePendingFollowUp,
  sendFirstTurn,
  startFollowUp,
} from "./follow-up.mjs";
import { createWorkspaceDom, watchFindingFilter } from "./workspace-dom.mjs";
import { requestKey } from "./workspace-view.mjs";
import {
  describeSourceError,
  findUnavailableRoots,
  isRootUnavailable,
} from "./source-availability.mjs";

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
  steeringError: null,
  confirmedSteering: [],
  measurementSummary: null,
  activity: [],
  contextHits: [],
  evidence: null,
  evidenceError: null,
  unavailableRoots: null,
  lastSearch: null,
  searchError: null,
  searchGeneration: 0,
  latestQuery: null,
  evidenceGeneration: 0,
  pendingRequests: [],
  requestItems: new Map(),
  selectedNodeId: null,
  findingFilter: { text: "", kind: "" },
  blackboardTruncated: false,
  answerMeasurement: null,
  liveTurnId: null,
  // Unknown until thread/read or a live turn event says otherwise; never assumed idle.
  execution: createExecution(),
  memorySummary: null,
  recap: null,
  recapDismissed: false,
  receipts: [],
  memory: null,
  memoryEditing: null,
  answering: new Set(),
  runGeneration: 0,
  activityTruncated: false,
  loading: true,
  busyAction: null,
  error: null,
  notice: null,
};

let refreshTimer;
const refresh = createRefreshGate(refreshWorkspace);
const view = createWorkspaceDom(app);
const sourceSearch = createSourceSearch({
  state,
  rpc,
  action: (label, operation) => action(label, operation),
  render: (sections) => render(sections),
  refresh: () => refresh(),
  projectId,
});
const drafts = view.drafts;
const readMemorySummary = createSummaryReader(rpc, { threadId, storage: sessionStorage });

async function boot() {
  render();
  subscribe(handleEvent, { threadId, projectId });
  try {
    // The session's memory watermark is set before the opening turn can save anything.
    state.memorySummary = await readMemorySummary();
    render(["memory-status"]);
    await ensureRun();
    await refresh();
    // The return card is read once per page load; it describes where things stood on arrival.
    state.recap = await readRecap(rpc, threadId);
    render(["recap"]);
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
  state.run = runResponse.run;
  // A follow-up whose start or first turn was never confirmed is finished before anything else.
  if (readPendingFollowUp(sessionStorage, { projectId, threadId })) {
    state.busyAction = "Finishing the follow-up";
    render(["notices"]);
    await resumePendingFollowUp({ state, rpc, storage: sessionStorage });
    selectedMode = state.run.mode;
  }
  state.unavailableRoots = await findUnavailableRoots(rpc, state.project.roots);
  // Indexing a missing folder cannot succeed; the header explains the condition instead.
  if (needsProjectRefresh(status) && !allRootsUnavailable()) {
    state.busyAction = status.initialized
      ? "Retrying the incomplete project index"
      : "Indexing the selected project";
    render(["notices"]);
    await rpc("contextMap/refresh", { projectId });
  }
  if (state.run) {
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
      render(["notices"]);
      await sendInitialTurn();
    }
    return;
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
  render(["notices"]);
  await sendInitialTurn();
}

async function refreshWorkspace() {
  // A run that changes while this refresh is in flight (a follow-up starting) makes its reads
  // obsolete; they are discarded and the refresh gate runs again.
  const generation = state.runGeneration;
  state.loading = !state.project;
  // An action's error stays visible until the user acts again; refreshes clear only their own.
  if (state.refreshFailed) {
    state.error = null;
    state.refreshFailed = false;
  }
  render(["notices"]);
  // What is running and what memory holds are reconciled even when an unrelated panel read
  // fails; neither reader rejects.
  const executionToken = beginSnapshot(state.execution);
  const statusReads = Promise.all([readExecutionSnapshot(rpc, threadId), readMemorySummary()]);
  const applyStatus = async () => {
    const [snapshot, memorySummary] = await statusReads;
    // A run that changed meanwhile makes these reads obsolete; a newer live event wins.
    if (generation !== state.runGeneration) return;
    applySnapshot(state.execution, executionToken, snapshot);
    state.memorySummary = memorySummary;
    render(["header", "memory-status"]);
  };
  let reads;
  try {
    reads = await Promise.all([
      rpc("project/read", { projectId }),
      rpc("statefulRun/read", runReadParams()),
      rpc("projectIntelligence/status", { projectId }),
      readHierarchy(),
      rpc("blackboard/query", { projectId, text: null, limit: 50 }),
      readMemory(),
      state.run
        ? rpc("obligation/list", { runId: state.run.id, cursor: null, limit: 100 })
        : { data: [] },
      state.run
        ? rpc("steering/list", { runId: state.run.id, cursor: null, limit: 100 })
        : { data: [] },
      rpc("statefulMeasurement/summary", { projectId, limit: 100 }),
      rpc("thread/items/list", {
        threadId,
        cursor: null,
        limit: 100,
        sortDirection: "desc",
      }).catch((error) => {
        if (error.code === -32601) return { data: [] };
        throw error;
      }),
      findUnavailableRoots(rpc, state.project?.roots),
    ]);
  } finally {
    await applyStatus();
  }
  const [
    project,
    run,
    status,
    hierarchy,
    blackboard,
    memory,
    obligations,
    steering,
    measurementSummary,
    activity,
    unavailableRoots,
  ] = reads;
  if (generation !== state.runGeneration) {
    refresh();
    return;
  }
  state.project = project.project;
  state.run = run.run;
  state.recovery = run.recovery;
  state.status = status;
  state.hierarchy = hierarchy;
  state.blackboard = blackboard.data;
  state.blackboardTruncated = blackboard.truncated === true;
  state.memory = memory;
  state.obligations = obligations.data;
  // The list is read oldest first and capped, so keep steering confirmed in this page view,
  // marked stale when the capped read no longer includes it.
  state.steering = [
    ...steering.data,
    ...state.confirmedSteering
      .filter((saved) => !steering.data.some((item) => item.id === saved.id))
      .map((saved) => ({ ...saved, statusStale: true })),
  ];
  state.measurementSummary = measurementSummary.summary;
  state.unavailableRoots = unavailableRoots;
  // A missing-folder error beside a source clears once the folder is back.
  if (state.evidenceError?.rootUnavailable && rootRecovered(state.evidenceError.root, unavailableRoots)) {
    state.evidenceError = null;
  }
  state.activity = activity.data.reverse();
  state.activityTruncated = Boolean(activity.nextCursor);
  state.answerMeasurement = await findTurnMeasurement(rpc, {
    projectId,
    threadId,
    turnId: latestAnswerTurn(state),
    known: state.answerMeasurement,
  });
  state.loading = false;
  state.busyAction = null;
  render();
}

// Two pages at most; a failure is shown in the panel rather than failing the whole refresh.
async function readMemory() {
  try {
    const items = [];
    let cursor = null;
    for (let page = 0; page < 2; page += 1) {
      const response = await rpc("statefulMemory/read", { threadId, cursor, limit: 50 });
      items.push(...response.data);
      cursor = response.nextCursor ?? null;
      if (!cursor) break;
    }
    return { items, more: Boolean(cursor), error: null };
  } catch (error) {
    return { items: [], more: false, error: error.message };
  }
}

function memoryItem(entryId) {
  return state.memory?.items.find((item) => item.entryId === entryId) ?? null;
}

// An error names its source's root when known; otherwise any missing root keeps it.
function rootRecovered(root, unavailableRoots) {
  return root ? !isRootUnavailable(unavailableRoots, root) : unavailableRoots.size === 0;
}

function allRootsUnavailable() {
  const roots = state.project?.roots ?? [];
  return roots.length > 0 && state.unavailableRoots?.size === roots.length;
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

// Updates the named slots (all by default); before the first mount it renders the loading
// screen or mounts the workspace.
const baseTitle = document.title;

function render(sections) {
  view.update(state, sections);
  document.title = documentTitle(baseTitle, state.pendingRequests.length);
}

function handleEvent(message) {
  const effect = applyWorkspaceEvent(state, message);
  if (effect.turnStarted) view.startTurn(effect.turnStarted);
  // A new turn's answer has no record yet; hide the previous answer's provenance meanwhile.
  const liveTurn =
    effect.turnStarted ?? effect.delta?.turnId ?? effect.completedMessage?.turnId ?? null;
  if (liveTurn && liveTurn !== state.liveTurnId) {
    state.liveTurnId = liveTurn;
    render(["activity"]);
  }
  if (effect.delta) view.pushDelta(effect.delta);
  if (effect.completedMessage) view.completeMessage(effect.completedMessage);
  if (effect.sections.length) render(effect.sections);
  if (effect.refresh) {
    clearTimeout(refreshTimer);
    refreshTimer = setTimeout(
      () =>
        refresh().catch((error) => {
          // Keep a displayed action error; report the background failure beside it.
          if (state.error && !state.refreshFailed) {
            state.notice = `Background refresh failed: ${error.message}`;
            render(["notices"]);
            return;
          }
          fail(error);
          state.refreshFailed = true;
        }),
      180,
    );
  }
}

watchFindingFilter(app, (filter) => {
  state.findingFilter = filter;
  render(["findings"]);
});

app.addEventListener("submit", async (event) => {
  event.preventDefault();
  const form = event.target;
  try {
    if (form.id === "steering-form") {
      const saved = await submitSteering({
        state,
        control: form.querySelector('[name="steering"]'),
        drafts,
        rpc,
        action,
      });
      render(["steering-status", "steering-list"]);
      if (saved) await refresh();
    } else if (form.id === "continue-form") {
      const mode = form.querySelector('[name="followup-mode"]').value;
      await drafts.submit(form.querySelector('[name="followup"]'), (goal) =>
        action("Starting the follow-up", () => continueThread(goal, mode)),
      );
    } else if (form.id === "message-form") {
      await drafts.submit(form.querySelector('[name="message"]'), (input) =>
        action("Sending instruction", () => sendTurn(input)),
      );
    } else if (form.id === "mode-form") {
      await drafts.submit(
        form.querySelector('[name="mode"]'),
        async (mode) => {
          if (mode === state.run.mode) return;
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
        },
        { clear: false },
      );
      // Refresh after the submission settles, so the clean selector shows the persisted mode.
      await refresh();
    } else if (form.dataset.memoryCorrect) {
      const content = form.querySelector('[name="content"]').value.trim();
      if (!content) return;
      const response = await action("Saving the correction", () =>
        rpc("statefulMemory/correct", {
          threadId,
          entryId: form.dataset.memoryCorrect,
          expectedRevision: Number(form.dataset.revision),
          content,
        }),
      );
      state.memoryEditing = null;
      state.notice = `Corrected: "${response.item.content}".`;
      await refresh();
    } else if (form.dataset.requestKey) {
      await answerUserRequest(form);
    } else if (form.id === "context-search") {
      // The query stays in the field so it continues to describe the results below it.
      const text = form.querySelector('[name="query"]').value.trim();
      if (!text) return;
      await sourceSearch.search(text);
    }
  } catch (error) {
    fail(error);
  }
});

app.addEventListener("click", async (event) => {
  const button = event.target.closest("button[data-action]");
  if (!button) return;
  const requestCard = button.closest("[data-request-key]");
  try {
    switch (button.dataset.action) {
      case "refresh":
        await action("Refreshing workspace", refresh);
        break;
      case "refresh-map":
        await sourceSearch.refreshMap();
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
        await readEvidence(button.dataset.entryId, lineRange, button.dataset.errorKey, button.dataset.root);
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
        view.selectNode(state);
        break;
      case "show-requests":
        view.showRequests();
        break;
      case "memory-correct":
        state.memoryEditing = button.dataset.entryId;
        render(["memory"]);
        break;
      case "recap-dismiss":
        state.recapDismissed = true;
        render(["recap"]);
        break;
      case "memory-cancel":
        state.memoryEditing = null;
        render(["memory"]);
        break;
      case "memory-forget": {
        const item = memoryItem(button.dataset.entryId);
        if (!item) break;
        await action("Forgetting", () =>
          rpc("statefulMemory/forget", {
            threadId,
            entryId: item.entryId,
            expectedRevision: Number(button.dataset.revision),
          }),
        );
        state.notice = `Forgot: "${item.content}". It stays in history but no longer applies.`;
        await refresh();
        break;
      }
      case "approve":
      case "decline":
        await answerApproval(
          requestCard?.dataset.requestKey,
          button.dataset.action,
        );
        break;
    }
  } catch (error) {
    fail(error);
  }
});

// Evidence failures are expected conditions (a changed or missing source): explain them beside
// the control that was used, and learn from a missing root.
async function readEvidence(entryId, lineRange, errorKey, root = null) {
  state.evidenceError = null;
  // A map refresh while this read is pending makes its answer obsolete.
  const generation = state.evidenceGeneration;
  try {
    const evidence = await action("Verifying exact evidence", () =>
      rpc("evidence/read", {
        projectId,
        contextMapEntryId: entryId,
        lineRange,
        maxBytes: 32768,
      }),
    );
    if (generation !== state.evidenceGeneration) return;
    state.evidence = evidence;
  } catch (error) {
    if (generation !== state.evidenceGeneration) return;
    const described = describeSourceError(error.message);
    state.evidence = null;
    state.evidenceError = {
      key: errorKey,
      message: described.message,
      rootUnavailable: described.rootUnavailable,
      root,
    };
    if (described.rootUnavailable) {
      state.unavailableRoots = await findUnavailableRoots(rpc, state.project?.roots);
    }
  }
  render(["header", "intelligence", "routing-results", "findings"]);
}

async function action(label, operation) {
  state.busyAction = label;
  state.error = null;
  state.refreshFailed = false;
  render(["notices"]);
  try {
    return await operation();
  } finally {
    state.busyAction = null;
    render(["notices"]);
  }
}

async function sendTurn(text) {
  return rpc("turn/start", {
    threadId,
    input: [{ type: "text", text, text_elements: [] }],
  });
}

async function continueThread(goal, mode) {
  try {
    await startFollowUp({
      state,
      rpc,
      storage: sessionStorage,
      goal,
      mode,
      onRunChanged: () => render(),
    });
  } catch (error) {
    // A refused follow-up's text moves to the running workspace's instruction box.
    if (error.refusedText) {
      await refresh();
      const box = app.querySelector('#message-form [name="message"]');
      if (box && !box.value.trim()) box.value = error.refusedText;
    }
    throw error;
  } finally {
    if (state.run) selectedMode = state.run.mode;
    await refresh();
  }
}

async function sendInitialTurn() {
  await sendFirstTurn({
    rpc,
    storage: sessionStorage,
    threadId,
    run: state.run,
    text: initialGoal,
  });
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

async function answerApproval(key, actionName) {
  const request = state.pendingRequests.find(
    (item) => requestKey(item.id) === key,
  );
  // A second click while the first answer is on its way sends nothing.
  if (!request || state.answering.has(key)) return;
  state.answering.add(key);
  const card = app.querySelector(`[data-request-key="${CSS.escape(key)}"]`);
  const buttons = card ? [...card.querySelectorAll("button[data-action]")] : [];
  for (const button of buttons) button.disabled = true;
  const pressed = buttons.find((button) => button.dataset.action === actionName);
  const label = pressed?.textContent;
  if (pressed) pressed.textContent = "Sending…";
  try {
    await reply(threadId, {
      id: request.id,
      result: { decision: actionName === "approve" ? "accept" : "decline" },
    });
  } catch (error) {
    for (const button of buttons) button.disabled = false;
    if (pressed) pressed.textContent = label;
    throw new Error(`Your answer was not delivered; try again. ${error.message}`);
  } finally {
    state.answering.delete(key);
  }
  state.pendingRequests = state.pendingRequests.filter(
    (item) => item.id !== request.id,
  );
  render(["requests", "header"]);
}

async function answerUserRequest(form) {
  const request = state.pendingRequests.find(
    (item) => requestKey(item.id) === form.dataset.requestKey,
  );
  if (!request) return;
  const data = new FormData(form);
  const answers = Object.fromEntries(
    request.params.questions.map((question) => [
      question.id,
      { answers: [data.get(question.id)?.toString() ?? ""] },
    ]),
  );
  await reply(threadId, { id: request.id, result: { answers } });
  state.pendingRequests = state.pendingRequests.filter(
    (item) => item.id !== request.id,
  );
  render(["requests", "header"]);
}

// Records an action's error; a background refresh marks its own errors after calling this.
function fail(error) {
  state.refreshFailed = false;
  state.loading = false;
  state.busyAction = null;
  state.error = error.message;
  render(["notices"]);
}

boot();
