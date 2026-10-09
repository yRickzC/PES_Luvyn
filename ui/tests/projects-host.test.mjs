import { test } from "node:test";
import assert from "node:assert/strict";

globalThis.window = {};
globalThis.location = { hash: "", pathname: "/", search: "" };
globalThis.localStorage = { getItem: () => "2" };
globalThis.sessionStorage = { removeItem: () => {} };
const { api, ApiConnectionError, connectGoogleDrive } =
  await import("../src/api.ts");

test("network failure is actionable, never retries writes, and allows recovery", async () => {
  let attempts = 0;
  globalThis.fetch = async () => {
    attempts++;
    throw new TypeError("Failed to fetch");
  };
  await assert.rejects(
    api("project-open", { path: "example" }),
    ApiConnectionError,
  );
  assert.equal(attempts, 1);
  globalThis.fetch = async () => Response.json({ recent: [] });
  assert.deepEqual(await api("projects"), { recent: [] });
});

test("browser login reserves a tab in the current browser before requesting OAuth", async () => {
  const order = [];
  let destination;
  const tab = {
    opener: {},
    location: {
      replace: (url) => {
        destination = url;
      },
    },
    close: () => assert.fail("Unexpected close"),
  };
  window.open = (url, target) => {
    order.push("tab");
    assert.equal(url, "about:blank");
    assert.equal(target, "_blank");
    return tab;
  };
  globalThis.fetch = async () => {
    order.push("request");
    return Response.json({
      authorization_url: "https://accounts.google.com/o/oauth2/v2/auth?test",
      pending: true,
    });
  };
  await connectGoogleDrive(true);
  assert.deepEqual(order, ["tab", "request"]);
  assert.ok(destination.startsWith("https://accounts.google.com/"));
  assert.equal(tab.opener, null);
});

test("failed OAuth preparation closes the blank tab and blocked popups never start OAuth", async () => {
  let closed = false;
  window.open = () => ({
    close: () => {
      closed = true;
    },
  });
  globalThis.fetch = async () => {
    throw new TypeError("Failed to fetch");
  };
  await assert.rejects(connectGoogleDrive(true), ApiConnectionError);
  assert.equal(closed, true);
  window.open = () => null;
  globalThis.fetch = async () =>
    assert.fail("OAuth must not start if popup blocked");
  await assert.rejects(connectGoogleDrive(true), /Permita abrir/);
});

test("desktop login leaves browser launching to its host", async () => {
  window.open = () => assert.fail("Desktop must not create a WebView popup");
  globalThis.fetch = async () =>
    Response.json({ pending: true, authorization_url: null });
  assert.equal((await connectGoogleDrive(false)).pending, true);
});
