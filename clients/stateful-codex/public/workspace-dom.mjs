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
  const hierarchyMemo = { source: null, signature: null, html: null };
  const live = {
    turnId: null,
    retiredTurns: new Set(),
    pending: new Map(),
    scheduled: false,
    items: new Map(),
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
      cardHtml.set(cards[index], renderRequestCard(request)),
    );
    mounted = true;
    if (live.pending.size) scheduleFlush();
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

  function update(state, names = ALL_SLOTS) {
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
      renderRequestCard(request),
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
      if (current !== card) list.insertBefore(card, current ?? null);
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
    for (const text of live.items.values()) text.parentNode?.remove();
    live.items.clear();
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
    live.pending.clear();
    trimLive();
    root.querySelector("[data-live]").hidden = live.length === 0;
  }

  function trimLive() {
    let excess = live.length - liveLimit;
    if (excess <= 0) return;
    root.querySelector("[data-live-truncated]").hidden = false;
    for (const [itemId, text] of live.items) {
      if (excess <= 0) break;
      const removed = Math.min(excess, text.data.length);
      if (removed === text.data.length) {
        text.parentNode.remove();
        live.items.delete(itemId);
      } else {
        text.deleteData(0, removed);
      }
      excess -= removed;
      live.length -= removed;
    }
  }

  function fromHtml(html) {
    const container = document.createElement("div");
    container.innerHTML = html;
    return container.children[0];
  }

  return { update, selectNode, startTurn, pushDelta };
}

// Drafts belong to the user: a successful submission clears a field only when nobody edited
// it while the request was pending, and a failed one leaves it untouched.
export function createDraftTracker(root) {
  const versions = new WeakMap();
  root.addEventListener("input", (event) => {
    versions.set(event.target, (versions.get(event.target) ?? 0) + 1);
  });
  return {
    async submit(control, operation) {
      const value = control.value.trim();
      if (!value) return;
      const version = versions.get(control) ?? 0;
      await operation(value);
      if ((versions.get(control) ?? 0) === version) control.value = "";
    },
  };
}

function elementKey(element) {
  const attributes = ["id", "name", "data-action", "data-entry-id", "data-node-id"]
    .map((name) => [name, element.getAttribute(name)])
    .filter(([, value]) => value !== null);
  if (!attributes.length) return null;
  return {
    selector: element.tagName.toLowerCase(),
    matches: (candidate) =>
      attributes.every(([name, value]) => candidate.getAttribute(name) === value),
  };
}
