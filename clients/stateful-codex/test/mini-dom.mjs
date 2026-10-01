// A deliberately small DOM for node tests of the browser client. It parses the HTML the
// client's own renderers produce, supports the simple selectors the client uses, bubbles
// events to listeners, and keeps node identity, focus and text nodes observable. It is not a
// general HTML implementation.

const VOID_ELEMENTS = new Set(["br", "hr", "img", "input", "link", "meta"]);
const BOOLEAN_PROPERTIES = ["hidden", "disabled", "required", "open"];
const ENTITIES = { amp: "&", lt: "<", gt: ">", quot: '"', "#39": "'" };

class Node {
  constructor(ownerDocument) {
    this.ownerDocument = ownerDocument;
    this.parentNode = null;
    this.childNodes = [];
  }

  // Assertion failures print a short description instead of walking the whole node graph.
  [Symbol.for("nodejs.util.inspect.custom")]() {
    return this.nodeType === 3
      ? `#text ${JSON.stringify(this.data.slice(0, 40))}`
      : `<${this.tagName.toLowerCase()}${this.id ? `#${this.id}` : ""}${this.name ? ` name=${this.name}` : ""}>`;
  }

  get isConnected() {
    let node = this;
    while (node.parentNode) node = node.parentNode;
    return node === this.ownerDocument.body;
  }

  get textContent() {
    return this.childNodes.map((child) => child.textContent).join("");
  }

  set textContent(value) {
    this.replaceChildren(this.ownerDocument.createTextNode(String(value)));
  }

  append(...nodes) {
    for (const node of nodes) this.insertBefore(asNode(this, node), null);
  }

  insertBefore(node, reference) {
    node.remove();
    node.parentNode = this;
    const index = reference ? this.childNodes.indexOf(reference) : -1;
    if (index < 0) this.childNodes.push(node);
    else this.childNodes.splice(index, 0, node);
    return node;
  }

  replaceChildren(...nodes) {
    for (const child of [...this.childNodes]) child.remove();
    this.append(...nodes);
  }

  remove() {
    if (!this.parentNode) return;
    const siblings = this.parentNode.childNodes;
    siblings.splice(siblings.indexOf(this), 1);
    this.parentNode = null;
    const active = this.ownerDocument.activeElement;
    if (active && (active === this || this.contains?.(active))) {
      this.ownerDocument.activeElement = this.ownerDocument.body;
    }
  }
}

export class Text extends Node {
  constructor(ownerDocument, data) {
    super(ownerDocument);
    this.nodeType = 3;
    this.data = data;
    this.appendCount = 0;
  }

  get textContent() {
    return this.data;
  }

  set textContent(value) {
    this.data = String(value);
  }

  appendData(value) {
    this.data += value;
    this.appendCount += 1;
  }

  deleteData(offset, count) {
    this.data = this.data.slice(0, offset) + this.data.slice(offset + count);
  }
}

export class Element extends Node {
  constructor(ownerDocument, tagName) {
    super(ownerDocument);
    this.nodeType = 1;
    this.tagName = tagName.toUpperCase();
    this.attributes = new Map();
    this.listeners = new Map();
    this.innerHTMLWrites = 0;
    this.dataset = new Proxy(
      {},
      {
        get: (_, key) =>
          typeof key === "string"
            ? (this.getAttribute(`data-${kebab(key)}`) ?? undefined)
            : undefined,
        set: (_, key, value) => {
          this.setAttribute(`data-${kebab(key)}`, value);
          return true;
        },
      },
    );
    for (const property of BOOLEAN_PROPERTIES) {
      Object.defineProperty(this, property, {
        get: () => this.hasAttribute(property),
        set: (value) => this.toggleAttribute(property, Boolean(value)),
      });
    }
  }

  get id() {
    return this.getAttribute("id") ?? "";
  }

  get name() {
    return this.getAttribute("name") ?? "";
  }

  get className() {
    return this.getAttribute("class") ?? "";
  }

  get classList() {
    const element = this;
    const names = () => element.className.split(/\s+/).filter(Boolean);
    return {
      contains: (name) => names().includes(name),
      toggle(name, force) {
        const present = names().includes(name);
        const next = force ?? !present;
        if (next !== present) {
          element.setAttribute(
            "class",
            (next ? [...names(), name] : names().filter((item) => item !== name)).join(" "),
          );
        }
        return next;
      },
    };
  }

  get children() {
    return this.childNodes.filter((child) => child.nodeType === 1);
  }

  get value() {
    if (this.tagName === "SELECT") {
      const options = this.querySelectorAll("option");
      const selected = options.find((option) => option.selected) ?? options[0];
      return selected ? selected.value : "";
    }
    if (this.tagName === "OPTION") return this.getAttribute("value") ?? this.textContent;
    if (this.dirtyValue !== undefined) return this.dirtyValue;
    if (this.tagName === "TEXTAREA") return this.textContent;
    return this.getAttribute("value") ?? "";
  }

  set value(value) {
    if (this.tagName === "SELECT") {
      for (const option of this.querySelectorAll("option")) {
        option.selected = option.value === String(value);
      }
      return;
    }
    this.dirtyValue = String(value);
  }

  get selected() {
    return this.selectedness ?? this.hasAttribute("selected");
  }

  set selected(value) {
    this.selectedness = Boolean(value);
  }

  getAttribute(name) {
    return this.attributes.has(name) ? this.attributes.get(name) : null;
  }

  hasAttribute(name) {
    return this.attributes.has(name);
  }

  setAttribute(name, value) {
    this.attributes.set(name, String(value));
  }

  removeAttribute(name) {
    this.attributes.delete(name);
  }

  toggleAttribute(name, force) {
    const next = force ?? !this.hasAttribute(name);
    if (next) this.setAttribute(name, "");
    else this.removeAttribute(name);
    return next;
  }

  get innerHTML() {
    return this.childNodes.map(serialize).join("");
  }

  set innerHTML(html) {
    this.innerHTMLWrites += 1;
    this.replaceChildren(...parse(this.ownerDocument, html));
  }

  contains(node) {
    for (let current = node; current; current = current.parentNode) {
      if (current === this) return true;
    }
    return false;
  }

  matches(selector) {
    return selector
      .split(",")
      .some((part) => matchesChain(this, part.trim().split(/\s+/)));
  }

  closest(selector) {
    for (let node = this; node?.nodeType === 1; node = node.parentNode) {
      if (node.matches(selector)) return node;
    }
    return null;
  }

  querySelectorAll(selector) {
    const found = [];
    const walk = (node) => {
      for (const child of node.children) {
        if (child.matches(selector)) found.push(child);
        walk(child);
      }
    };
    walk(this);
    return found;
  }

  querySelector(selector) {
    return this.querySelectorAll(selector)[0] ?? null;
  }

  addEventListener(type, listener) {
    this.listeners.set(type, [...(this.listeners.get(type) ?? []), listener]);
  }

  focus() {
    this.ownerDocument.activeElement = this;
  }

  // Bubble an event from this element. Returns the event so tests can await listener promises.
  dispatch(type, init = {}) {
    const event = {
      type,
      target: this,
      defaultPrevented: false,
      preventDefault() {
        this.defaultPrevented = true;
      },
      results: [],
      ...init,
    };
    for (let node = this; node; node = node.parentNode) {
      for (const listener of node.listeners?.get(type) ?? []) {
        event.results.push(listener(event));
      }
    }
    return event;
  }
}

export function createDocument() {
  const document = { activeElement: null };
  document.createElement = (tag) => new Element(document, tag);
  document.createTextNode = (data) => new Text(document, data);
  document.body = document.createElement("body");
  document.activeElement = document.body;
  return document;
}

// Simulate the user typing into a control: set its value and fire input, then change.
export function type(element, value) {
  element.value = value;
  element.dispatch("input");
}

function asNode(parent, node) {
  return typeof node === "string" ? parent.ownerDocument.createTextNode(node) : node;
}

function kebab(key) {
  return String(key).replace(/[A-Z]/g, (letter) => `-${letter.toLowerCase()}`);
}

function decode(text) {
  return text.replace(/&(amp|lt|gt|quot|#39);/g, (_, entity) => ENTITIES[entity]);
}

function encode(text) {
  return String(text).replace(
    /[&<>"']/g,
    (character) =>
      ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#39;" })[character],
  );
}

function parse(document, html) {
  const root = document.createElement("template");
  let current = root;
  const pattern = /<(\/?)([a-zA-Z0-9-]+)((?:\s+[^\s=>/]+(?:="[^"]*")?)*)\s*\/?>|([^<]+)/g;
  for (const match of html.matchAll(pattern)) {
    const [, closing, tag, attributes, text] = match;
    if (text !== undefined) {
      if (text.trim() || current.tagName === "TEXTAREA" || current.tagName === "PRE") {
        current.append(document.createTextNode(decode(text)));
      }
      continue;
    }
    if (closing) {
      for (let node = current; node && node !== root; node = node.parentNode) {
        if (node.tagName === tag.toUpperCase()) {
          current = node.parentNode;
          break;
        }
      }
      continue;
    }
    const element = document.createElement(tag);
    for (const [, name, value] of attributes.matchAll(/([^\s=>/]+)(?:="([^"]*)")?/g)) {
      element.setAttribute(name, decode(value ?? ""));
    }
    current.append(element);
    if (!VOID_ELEMENTS.has(tag.toLowerCase())) current = element;
  }
  return [...root.childNodes];
}

function serialize(node) {
  if (node.nodeType === 3) return encode(node.data);
  const tag = node.tagName.toLowerCase();
  const attributes = [...node.attributes]
    .map(([name, value]) => (value === "" ? ` ${name}` : ` ${name}="${encode(value)}"`))
    .join("");
  if (VOID_ELEMENTS.has(tag)) return `<${tag}${attributes}>`;
  return `<${tag}${attributes}>${node.childNodes.map(serialize).join("")}</${tag}>`;
}

function matchesChain(element, parts) {
  if (!matchesCompound(element, parts.at(-1))) return false;
  if (parts.length === 1) return true;
  for (let node = element.parentNode; node?.nodeType === 1; node = node.parentNode) {
    if (matchesChain(node, parts.slice(0, -1))) return true;
  }
  return false;
}

function matchesCompound(element, compound) {
  const tokens = compound.match(/^[a-zA-Z0-9]+|#[\w-]+|\.[\w-]+|\[[^\]]+\]/g) ?? [];
  return tokens.every((token) => {
    if (token[0] === "#") return element.id === token.slice(1);
    if (token[0] === ".") return element.classList.contains(token.slice(1));
    if (token[0] === "[") {
      const [, name, value] = token.match(/^\[([^=\]]+)(?:=["']?([^"'\]]*)["']?)?\]$/);
      return value === undefined
        ? element.hasAttribute(name)
        : element.getAttribute(name) === value;
    }
    return element.tagName === token.toUpperCase();
  });
}

// node:assert prints objects without custom inspectors, which walks the whole node graph;
// compare node identity with a short message instead.
export function assertSameNode(actual, expected, message = "node identity changed") {
  if (actual !== expected) {
    throw new Error(`${message}: ${describe(actual)} is not ${describe(expected)}`);
  }
}

function describe(node) {
  return node?.[Symbol.for("nodejs.util.inspect.custom")]?.() ?? String(node);
}
