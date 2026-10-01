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
    [String.raw`C:\work\gone`]: new Error("The system cannot find the path specified. (os error 3)"),
    "/srv/gone": new Error("No such file or directory (os error 2)"),
    [String.raw`C:\work\file.txt`]: { isDirectory: false },
    [String.raw`C:\work\denied`]: new Error("Access is denied. (os error 5)"),
  };
  const rpc = async (method, { path }) => {
    assert.equal(method, "fs/getMetadata");
    const response = responses[path];
    if (response instanceof Error) throw response;
    return response;
  };

  const unavailable = await findUnavailableRoots(
    rpc,
    Object.keys(responses).map((path) => ({ path })),
  );

  assert.deepEqual([...unavailable], ["C:/work/gone", "/srv/gone", "C:/work/file.txt"]);
  assert.equal(effectiveFreshness("current", unavailable, String.raw`C:\work\gone\ `.trim()), "sourceUnavailable");
  assert.equal(effectiveFreshness("current", unavailable, String.raw`C:\work\present`), "current");
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
  assert.equal(
    describeSourceError("evidence source is Stale; refresh and verify it before reading").message,
    "This file changed after it was indexed. Refresh the map, then verify it again.",
  );
  assert.equal(describeSourceError("something else").message, "something else");
});
