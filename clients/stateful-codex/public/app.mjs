import { rpc, subscribe } from "./rpc.mjs";
import { startSetup } from "./setup-controller.mjs";

startSetup({
  app: document.querySelector("#app"),
  rpc,
  subscribe,
  readForm: (form) => new FormData(form),
  storage: sessionStorage,
  navigate: (path) => window.location.assign(path),
  newId: () => crypto.randomUUID(),
}).boot();
