import assert from "node:assert/strict";
import test from "node:test";

import {
  describeSourceError,
  effectiveFreshness,
  findUnavailableRoots,
} from "../public/source-availability.mjs";

test("only definitely missing folders are reported unavailable", async () => {
  const responses = {
    [String.raw`C:\work\present`]: { isDirectory: true },
    [String.raw`C:\Work\Gone`]: new Error("The system cannot find the path specified. (os error 3)"),
    "/srv/Gone": new Error("No such file or directory (os error 2)"),
    [String.raw`C:\work\file.txt`]: { isDirectory: false },
    [String.raw`C:\work\denied`]: new Error("Access is denied. (os error 5)"),
    [String.raw`\\share\stalled`]: "never answers",
  };
  const rpc = async (method, { path }) => {
    assert.equal(method, "fs/getMetadata");
    const response = responses[path];
    if (response === "never answers") return new Promise(() => {});
    if (response instanceof Error) throw response;
    return response;
  };

  const unavailable = await findUnavailableRoots(
    rpc,
    Object.keys(responses).map((path) => ({ path })),
    { timeoutMs: 20 },
  );

  assert.deepEqual([...unavailable], ["c:/work/gone", "/srv/Gone", "c:/work/file.txt"]);
  // Windows paths compare without regard to case or separators; POSIX paths keep their case.
  assert.equal(effectiveFreshness("current", unavailable, "c:/WORK/gone/"), "sourceUnavailable");
  assert.equal(effectiveFreshness("current", unavailable, String.raw`C:\work\present`), "current");
  assert.equal(effectiveFreshness("current", unavailable, "/srv/gone"), "current");
});

test("exact-source errors are explained in plain language", () => {
  assert.deepEqual(
    describeSourceError(
      "evidence root is unavailable: The system cannot find the path specified. (os error 3)",
    ),
    {
      rootUnavailable: true,
      message:
        "The project folder can't be found on disk, so this source can't be checked. Restore the folder at its recorded path, then refresh the map.",
    },
  );
  // Access failures are not reported as a missing folder.
  assert.deepEqual(describeSourceError("evidence root is unavailable: Access is denied. (os error 5)"), {
    rootUnavailable: false,
    message:
      "The project folder can't be read right now (Access is denied. (os error 5)). Check that it is reachable and that you have access, then try again.",
  });
  assert.equal(
    describeSourceError("evidence source is unavailable: Access is denied. (os error 5)").message,
    "This file can't be read right now (Access is denied. (os error 5)). Check that you have access to it, then try again.",
  );
  assert.equal(
    describeSourceError("evidence source is Stale; refresh and verify it before reading").message,
    "This file changed after it was indexed. Refresh the map, then verify it again.",
  );
  assert.equal(describeSourceError("something else").message, "something else");
});
