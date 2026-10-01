import {
  LIVE_TAIL_CHARACTERS,
  WORKSPACE_SLOTS,
  isLoadingScreen,
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
const SCROLLERS = [".tree", ".finding-list", ".activity-list", ".packet"];

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
    if (live.pending.size) scheduleFlush();
  }

  // Show the persisted mode unless the user has edited the selector since it was last clean.
  function syncMode(state) {
    const select = slotElements.get("mode-form").querySelector('[name="mode"]');
    if (select && state.run && drafts.isClean(select)) select.value = state.run.mode;
  }

  // Replace a slot's content while keeping the user's place: focus, scroll and open details.
  function replaceSlot(name, html) {
    const element = slotElements.get(name);
    const active = document.activeElement;
    const focusKey =
      active && active !== element && element.contains(active)
        ? elementKey(active)
        : null;
    const scrolls = SCROLLERS.flatMap((selector) =>
      [...element.querySelectorAll(selector)].map((node, index) => [
        selector,
        index,
        node.scrollTop,
      ]),
    ).filter(([, , top]) => top);
    const open = [...element.querySelectorAll("details")].map((details) => details.open);
    element.innerHTML = html;
    for (const [selector, index, top] of scrolls) {
      const node = element.querySelectorAll(selector)[index];
      if (node) node.scrollTop = top;
    }
    [...element.querySelectorAll("details")].forEach((details, index) => {
      if (open[index]) details.open = true;
    });
    if (focusKey) {
      [...element.querySelectorAll(focusKey.selector)].find(focusKey.matches)?.focus();
    }
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
        if (card) card.remove();
        card = fresh;
        cardHtml.set(card, html);
      }
      const current = list.children[index];
      if (current !== card) moveKeepingFocus(list, card, current ?? null);
    });
    drawer.hidden = wanted.length === 0;
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
    live.arrivals = [];
    for (const text of live.items.values()) text.parentNode?.remove();
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
    if (!live.pending.has(key)) live.pending.set(key, []);
    live.pending.get(key).push(delta);
    recordChunk(live.arrivals, key, delta.length);
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
    trimLive();
    root.querySelector("[data-live]").hidden = live.length === 0;
  }

  function trimLive() {
    let excess = live.length - liveLimit;
    if (excess <= 0) return;
    root.querySelector("[data-live-truncated]").hidden = false;
    // An item's oldest remaining characters are at the start of its text node.
    while (excess > 0) {
      const oldest = live.chunks[0];
      const text = live.items.get(oldest.itemId);
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

  return { update, selectNode, startTurn, pushDelta, drafts };
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

function elementKey(element) {
  const attributes = [
    "id",
    "name",
    "data-action",
    "data-entry-id",
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
