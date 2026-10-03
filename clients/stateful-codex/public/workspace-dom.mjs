import { renderMarkdown } from "./markdown.mjs";
import {
  LIVE_TAIL_CHARACTERS,
  WORKSPACE_SLOTS,
  isLoadingScreen,
  pendingSummary,
  renderRequestCard,
  renderWorkspace,
  renderWorkspaceShell,
  requestKey,
} from "./workspace-view.mjs";

// Every slot. The hierarchy slot is memoized on its data, so listing it here is cheap, and
// node selection is applied by attribute (selectNode) rather than by re-rendering the tree.
export const ALL_SLOTS = Object.keys(WORKSPACE_SLOTS);
// A full update also reconciles request cards, whose content can depend on refreshed activity.
const FULL_UPDATE = [...ALL_SLOTS, "requests"];
const SCROLLERS = [".tree", ".finding-list", ".activity-list", ".packet", ".approval-diff"];

// Mounts the workspace once and then edits it in place: named slots are replaced only when
// their markup changes, request cards are reconciled by request ID, and streamed text is
// appended to persistent text nodes once per animation frame.
export function createWorkspaceDom(
  root,
  {
    slots = WORKSPACE_SLOTS,
    schedule = (callback) => requestAnimationFrame(callback),
    liveLimit = LIVE_TAIL_CHARACTERS,
  } = {},
) {
  const document = root.ownerDocument;
  const slotElements = new Map();
  const slotHtml = new Map();
  const cardHtml = new WeakMap();
  const drafts = createDraftTracker(root);
  const hierarchyMemo = { source: null, signature: null, html: null };
  const live = {
    turnId: null,
    retiredTurns: new Set(),
    pending: new Map(),
    // Finished agent messages to format in place on the next frame, by item.
    completed: new Map(),
    // Items of this turn whose final text has arrived; later deltas for them are dropped, even
    // after a formatted item has been trimmed away.
    finished: new Set(),
    // Item and length of each queued delta run, in arrival order, for the next frame.
    arrivals: [],
    scheduled: false,
    items: new Map(),
    // Appended chunks in arrival order, so trimming always drops the oldest text first.
    chunks: [],
    length: 0,
  };
  let mounted = false;
  let loadingHtml = null;

  function renderSlot(name, state) {
    if (name !== "hierarchy") return slots[name](state);
    // Rebuilding a large tree is the expensive path; skip it unless its inputs changed.
    if (hierarchyMemo.source !== state.hierarchy) {
      const signature = JSON.stringify([state.project?.name, state.hierarchy]);
      if (signature !== hierarchyMemo.signature) {
        hierarchyMemo.signature = signature;
        hierarchyMemo.html = slots.hierarchy(state);
      }
      hierarchyMemo.source = state.hierarchy;
    }
    return hierarchyMemo.html;
  }

  function mount(state) {
    root.innerHTML = renderWorkspaceShell(state, (name) => {
      const html = renderSlot(name, state);
      slotHtml.set(name, html);
      return html;
    });
    for (const element of root.querySelectorAll("[data-slot]")) {
      slotElements.set(element.dataset.slot, element);
    }
    const cards = root.querySelector("[data-request-list]").children;
    state.pendingRequests.forEach((request, index) =>
      cardHtml.set(cards[index], renderRequestCard(request, state)),
    );
    mounted = true;
    syncMode(state);
    syncSteering(state);
    if (live.pending.size || live.completed.size) scheduleFlush();
  }

  // Steering needs a run to attach to. The button, not the form markup, reflects that, so a
  // draft typed while the run is being prepared survives the run's arrival.
  function syncSteering(state) {
    const button = slotElements.get("steering-form").querySelector("button");
    if (button) button.disabled = !state.run;
  }

  // Show the persisted mode unless the user has edited the selector since it was last clean.
  function syncMode(state) {
    const select = slotElements.get("mode-form").querySelector('[name="mode"]');
    if (select && state.run && drafts.isClean(select)) select.value = state.run.mode;
  }

  // Replace a slot's content while keeping the user's place: focus, scroll and open details.
  function replaceSlot(name, html) {
    const element = slotElements.get(name);
    const place = capturePlace(element);
    element.innerHTML = html;
    restorePlace(element, place);
  }

  function update(state, names = FULL_UPDATE) {
    if (!mounted) {
      if (isLoadingScreen(state)) {
        const html = renderWorkspace(state);
        if (html !== loadingHtml) root.innerHTML = loadingHtml = html;
        return;
      }
      mount(state);
      return;
    }
    for (const name of names) {
      if (name === "requests") {
        reconcileRequests(state);
        continue;
      }
      const html = renderSlot(name, state);
      if (slotHtml.get(name) === html) continue;
      slotHtml.set(name, html);
      replaceSlot(name, html);
    }
    if (names.includes("mode-form")) syncMode(state);
    if (names.includes("steering-form")) syncSteering(state);
  }

  // Keep a card's element while its request is pending so a half-typed answer survives.
  function reconcileRequests(state) {
    const drawer = root.querySelector("[data-requests]");
    const list = drawer.querySelector("[data-request-list]");
    const existing = new Map(
      [...list.children].map((card) => [card.getAttribute("data-request-key"), card]),
    );
    const wanted = state.pendingRequests.map((request) => [
      requestKey(request.id),
      renderRequestCard(request, state),
    ]);
    const keep = new Set(wanted.map(([key]) => key));
    for (const [key, card] of existing) if (!keep.has(key)) card.remove();
    wanted.forEach(([key, html], index) => {
      let card = existing.get(key);
      if (!card || cardHtml.get(card) !== html) {
        const fresh = fromHtml(html);
        if (card) {
          const place = capturePlace(card);
          list.insertBefore(fresh, card);
          card.remove();
          restorePlace(fresh, place);
        }
        card = fresh;
        cardHtml.set(card, html);
      }
      const current = list.children[index];
      if (current !== card) moveKeepingFocus(list, card, current ?? null);
    });
    drawer.hidden = wanted.length === 0;
    const bar = root.querySelector("[data-pending-bar]");
    const summary = pendingSummary(wanted.length);
    const announcement = root.querySelector("[data-pending-announcement]");
    if (announcement.textContent !== summary) announcement.textContent = summary;
    const visible = bar.querySelector("span");
    if (visible.textContent !== summary) visible.textContent = summary;
    bar.hidden = wanted.length === 0;
  }

  // Bring the request cards into view and put the keyboard on the first one.
  function showRequests() {
    const drawer = root.querySelector("[data-requests]");
    if (drawer.hidden) return;
    drawer.scrollIntoView?.({ block: "start" });
    const target = drawer.querySelector("select, input, textarea, button:not([disabled])");
    if (!target) return;
    // The drawer scrolls internally; bring the control itself into view before focusing it.
    target.scrollIntoView?.({ block: "nearest" });
    target.focus({ preventScroll: true });
  }

  function selectNode(state) {
    const tree = slotElements.get("hierarchy");
    for (const row of tree.querySelectorAll('[data-selected="true"]')) {
      row.setAttribute("data-selected", "false");
    }
    if (state.selectedNodeId) {
      [...tree.querySelectorAll("[data-node-id]")]
        .find((row) => row.dataset.nodeId === state.selectedNodeId)
        ?.setAttribute("data-selected", "true");
    }
    update(state, ["findings"]);
  }

  function startTurn(turnId) {
    if (live.turnId && live.turnId !== turnId) live.retiredTurns.add(live.turnId);
    live.turnId = turnId;
    live.pending.clear();
    live.completed.clear();
    live.finished.clear();
    live.arrivals = [];
    for (const node of live.items.values()) liveItemElement(node)?.remove();
    live.items.clear();
    live.chunks = [];
    live.length = 0;
    if (mounted) {
      root.querySelector("[data-live-truncated]").hidden = true;
      root.querySelector("[data-live]").hidden = true;
    }
  }

  // Deltas are queued per item and written once per frame. A delta from a retired turn is
  // dropped; one from an unseen turn (for example after a reconnect) starts that turn.
  function pushDelta({ turnId = null, itemId = null, delta }) {
    if (!delta) return;
    if (turnId && turnId !== live.turnId) {
      if (live.retiredTurns.has(turnId)) return;
      startTurn(turnId);
    }
    const key = itemId ?? "";
    if (live.finished.has(key)) return;
    enqueue(key, delta);
  }

  function enqueue(key, delta) {
    if (!live.pending.has(key)) live.pending.set(key, []);
    live.pending.get(key).push(delta);
    recordChunk(live.arrivals, key, delta.length);
    scheduleFlush();
  }

  // A finished agent message replaces its streamed text with formatted Markdown, in place. One
  // too long for the live tail keeps its plain streamed text instead.
  function completeMessage({ turnId = null, itemId = null, text = "" }) {
    if (!text) return;
    if (turnId && turnId !== live.turnId) {
      if (live.retiredTurns.has(turnId)) return;
      startTurn(turnId);
    }
    const key = itemId ?? "";
    if (live.finished.has(key)) return;
    live.finished.add(key);
    // A message whose stream was missed arrives as one delta, so it keeps its arrival order.
    if (!live.items.has(key) && !live.pending.has(key)) enqueue(key, text);
    if (text.length > liveLimit) return;
    live.completed.set(key, text);
    scheduleFlush();
  }

  function scheduleFlush() {
    if (live.scheduled) return;
    live.scheduled = true;
    schedule(flush);
  }

  function flush() {
    live.scheduled = false;
    if (!mounted) return;
    const copy = root.querySelector("[data-live-copy]");
    for (const [itemId, chunks] of live.pending) {
      let text = live.items.get(itemId);
      if (!text) {
        const item = document.createElement("span");
        item.setAttribute("data-live-item", itemId);
        text = document.createTextNode("");
        item.append(text);
        copy.append(item);
        live.items.set(itemId, text);
      }
      const chunk = chunks.join("");
      text.appendData(chunk);
      live.length += chunk.length;
    }
    for (const { itemId, length } of live.arrivals) {
      recordChunk(live.chunks, itemId, length);
    }
    live.pending.clear();
    live.arrivals = [];
    for (const [itemId, text] of live.completed) formatItem(copy, itemId, text);
    live.completed.clear();
    trimLive();
    root.querySelector("[data-live]").hidden = live.length === 0;
  }

  // The formatted message keeps its place, and its accounting becomes one run at its first
  // arrival, so trimming still drops the oldest text first.
  function formatItem(copy, itemId, text) {
    const existing = live.items.get(itemId);
    let element;
    if (existing) {
      element = liveItemElement(existing);
    } else {
      element = document.createElement("span");
      element.setAttribute("data-live-item", itemId);
      copy.append(element);
    }
    element.setAttribute("data-rendered", "");
    element.innerHTML = renderMarkdown(text);
    live.items.set(itemId, element);
    const first = live.chunks.findIndex((chunk) => chunk.itemId === itemId);
    let previous = 0;
    live.chunks = live.chunks.filter((chunk) => {
      if (chunk.itemId !== itemId) return true;
      previous += chunk.length;
      return false;
    });
    const run = { itemId, length: text.length };
    if (first < 0) live.chunks.push(run);
    else live.chunks.splice(first, 0, run);
    live.length += text.length - previous;
  }

  function trimLive() {
    let excess = live.length - liveLimit;
    if (excess <= 0) return;
    root.querySelector("[data-live-truncated]").hidden = false;
    // An item's oldest remaining characters are at the start of its text node.
    while (excess > 0) {
      const oldest = live.chunks[0];
      const text = live.items.get(oldest.itemId);
      // Partial Markdown is meaningless, so a formatted message leaves the tail whole.
      if (text.nodeType !== TEXT_NODE) {
        text.remove();
        live.items.delete(oldest.itemId);
        live.chunks.shift();
        excess -= oldest.length;
        live.length -= oldest.length;
        continue;
      }
      const removed = Math.min(excess, oldest.length);
      text.deleteData(0, removed);
      oldest.length -= removed;
      if (!oldest.length) live.chunks.shift();
      if (!text.data.length) {
        text.parentNode.remove();
        live.items.delete(oldest.itemId);
      }
      excess -= removed;
      live.length -= removed;
    }
  }

  // Moving a node blurs it; restore focus and the text selection of a moved, focused control.
  function moveKeepingFocus(parent, node, before) {
    const active = document.activeElement;
    const focused = active && node.contains(active) ? active : null;
    const selection =
      focused && typeof focused.selectionStart === "number"
        ? [focused.selectionStart, focused.selectionEnd]
        : null;
    parent.insertBefore(node, before);
    if (focused && document.activeElement !== focused) {
      focused.focus();
      if (selection) focused.setSelectionRange(...selection);
    }
  }

  function fromHtml(html) {
    const container = document.createElement("div");
    container.innerHTML = html;
    return container.children[0];
  }

  return { update, selectNode, startTurn, pushDelta, completeMessage, showRequests, drafts };
}

const TEXT_NODE = 3;

// A live item is a streamed text node (inside its span) or a formatted span.
function liveItemElement(node) {
  return node.nodeType === TEXT_NODE ? node.parentNode : node;
}

// The findings filter lives outside the re-rendered findings slot and applies as the person
// types or picks a kind; it never submits.
export function watchFindingFilter(root, onFilter) {
  const apply = (event) => {
    const form = event.target.closest?.("#finding-filter");
    if (!form) return;
    onFilter({
      text: form.querySelector('[name="text"]').value,
      kind: form.querySelector('[name="kind"]').value,
    });
  };
  root.addEventListener("input", apply);
  root.addEventListener("change", apply);
}

// Consecutive text for the same item is one run; each run holds at least one character, so the
// list never outgrows the bounded tail.
function recordChunk(chunks, itemId, length) {
  const last = chunks.at(-1);
  if (last?.itemId === itemId) last.length += length;
  else chunks.push({ itemId, length });
}

// Drafts belong to the user: a successful submission settles a field (clearing text fields)
// only when nobody edited it while the request was pending, and a failed one leaves it
// untouched. A field is clean when it has no edits since it was last settled.
export function createDraftTracker(root) {
  const versions = new WeakMap();
  const settled = new WeakMap();
  const submitting = new WeakSet();
  const edited = (event) => {
    versions.set(event.target, (versions.get(event.target) ?? 0) + 1);
  };
  root.addEventListener("input", edited);
  root.addEventListener("change", edited);
  return {
    isClean(control) {
      return (versions.get(control) ?? 0) === (settled.get(control) ?? 0);
    },
    isSubmitting(control) {
      return submitting.has(control);
    },
    async submit(control, operation, { clear = true } = {}) {
      const value = control.value.trim();
      // One submission per field at a time; the field stays editable while it is pending.
      if (!value || submitting.has(control)) return;
      submitting.add(control);
      const version = versions.get(control) ?? 0;
      try {
        await operation(value);
      } finally {
        submitting.delete(control);
      }
      if ((versions.get(control) ?? 0) !== version) return;
      settled.set(control, version);
      if (clear) control.value = "";
    },
  };
}

// Where the user is inside a container that is about to be re-rendered.
function capturePlace(container) {
  const active = container.ownerDocument.activeElement;
  return {
    focusKey:
      active && active !== container && container.contains(active)
        ? elementKey(active)
        : null,
    // The caret or selection inside the focused text field, kept across the re-render.
    selection:
      active && typeof active.selectionStart === "number"
        ? [active.selectionStart, active.selectionEnd, active.selectionDirection]
        : null,
    scrolls: SCROLLERS.flatMap((selector) =>
      [...container.querySelectorAll(selector)].map((node, index) => [
        selector,
        index,
        node.scrollTop,
        node.scrollLeft,
      ]),
    ).filter(([, , top, left]) => top || left),
    open: [...container.querySelectorAll("details")].map((details) => details.open),
  };
}

function restorePlace(container, { focusKey, selection, scrolls, open }) {
  [...container.querySelectorAll("details")].forEach((details, index) => {
    if (open[index]) details.open = true;
  });
  for (const [selector, index, top, left] of scrolls) {
    const node = container.querySelectorAll(selector)[index];
    if (!node) continue;
    node.scrollTop = top;
    node.scrollLeft = left;
  }
  if (focusKey) {
    const target = [...container.querySelectorAll(focusKey.selector)].find(focusKey.matches);
    target?.focus();
    if (target && selection && typeof target.setSelectionRange === "function") {
      try {
        target.setSelectionRange(...selection);
      } catch {
        // A field whose type has no selection keeps the browser's caret.
      }
    }
  }
}

function elementKey(element) {
  const attributes = [
    "id",
    "name",
    "data-action",
    "data-entry-id",
    "data-revision",
    "data-node-id",
    "data-disclosure",
  ]
    .map((name) => [name, element.getAttribute(name)])
    .filter(([, value]) => value !== null);
  if (!attributes.length) return null;
  return {
    selector: element.tagName.toLowerCase(),
    matches: (candidate) =>
      attributes.every(([name, value]) => candidate.getAttribute(name) === value),
  };
}
