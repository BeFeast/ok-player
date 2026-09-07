import assert from "node:assert/strict";
import test from "node:test";

import {
  MENU_ID,
  NATIVE_HOST,
  dispatchActionClick,
  dispatchContextClick,
  registerBrowserIntegration,
  selectUrl,
} from "../extension/bridge.mjs";

function apiWithNativeReply(reply = { ok: true, status: "launch_requested" }) {
  const state = {
    createdTabs: [],
    messages: [],
    menu: null,
    contextListener: null,
    actionListener: null,
  };
  const api = {
    runtime: {
      lastError: null,
      getURL(path) {
        return `chrome-extension://test-extension/${path}`;
      },
      sendNativeMessage(host, message, callback) {
        state.messages.push({ host, message });
        if (reply instanceof Error) {
          this.lastError = { message: reply.message };
          callback(undefined);
          this.lastError = null;
          return;
        }
        callback(reply);
      },
      onInstalled: {
        addListener(listener) {
          state.installListener = listener;
        },
      },
    },
    tabs: {
      create(options) {
        state.createdTabs.push(options);
      },
    },
    contextMenus: {
      removeAll(callback) {
        callback();
      },
      create(options) {
        state.menu = options;
      },
      onClicked: {
        addListener(listener) {
          state.contextListener = listener;
        },
      },
    },
    action: {
      onClicked: {
        addListener(listener) {
          state.actionListener = listener;
        },
      },
    },
  };
  return { api, state };
}

test("a link click transfers the original Unicode URL exactly once", async () => {
  const { api, state } = apiWithNativeReply();
  const url = "https://example.test/watch/雪?q=a%26b&lang=日本語#chapter-2";
  const result = await dispatchContextClick(
    {
      menuItemId: MENU_ID,
      linkUrl: url,
      mediaType: "video",
      srcUrl: "https://cdn.example.test/other.mp4",
      pageUrl: "https://www.youtube.com/watch?v=wrong",
    },
    {},
    api,
  );

  assert.equal(result.ok, true);
  assert.deepEqual(state.messages, [{ host: NATIVE_HOST, message: { url } }]);
  assert.deepEqual(state.createdTabs, []);
});

test("a supported platform video keeps the page URL instead of its source", () => {
  const pageUrl = "https://m.youtube.com/watch?v=abc123&list=one#t=45";
  assert.equal(
    selectUrl(
      {
        mediaType: "video",
        srcUrl: "https://video.example.test/ephemeral.mp4?token=discard",
        pageUrl,
      },
      {},
    ),
    pageUrl,
  );
});

test("a blob video uses its ordinary page and a direct video uses its HTTP source", () => {
  const pageUrl = "https://news.example.test/story?item=7#player";
  assert.equal(
    selectUrl({ mediaType: "video", srcUrl: "blob:https://news.example.test/id", pageUrl }, {}),
    pageUrl,
  );
  assert.equal(
    selectUrl(
      {
        mediaType: "video",
        srcUrl: "https://media.example.test/movie.mp4?quality=hd#track",
        pageUrl,
      },
      {},
    ),
    "https://media.example.test/movie.mp4?quality=hd#track",
  );
});

test("page-menu and toolbar actions each send the current page once", async () => {
  const page = apiWithNativeReply();
  await dispatchContextClick(
    { menuItemId: MENU_ID, pageUrl: "https://x.com/example/status/42?lang=en#media" },
    {},
    page.api,
  );
  assert.deepEqual(page.state.messages.map((entry) => entry.message.url), [
    "https://x.com/example/status/42?lang=en#media",
  ]);

  const action = apiWithNativeReply();
  await dispatchActionClick(
    { url: "https://www.tiktok.com/@example/video/5?is_copy_url=1#view" },
    action.api,
  );
  assert.deepEqual(action.state.messages.map((entry) => entry.message.url), [
    "https://www.tiktok.com/@example/video/5?is_copy_url=1#view",
  ]);
});

for (const unsafeUrl of [
  "file:///tmp/movie.mp4",
  "javascript:alert(1)",
  "--fullscreen",
  " https://example.test/video",
  "https://example.test/video\n--fullscreen",
  "https://example.test/\ud800",
]) {
  test(`unsupported input is shown to the user without contacting the host: ${JSON.stringify(unsafeUrl)}`, async () => {
    const { api, state } = apiWithNativeReply();
    const result = await dispatchContextClick(
      { menuItemId: MENU_ID, linkUrl: unsafeUrl },
      {},
      api,
    );

    assert.equal(result.ok, false);
    assert.equal(state.messages.length, 0);
    assert.equal(state.createdTabs.length, 1);
    assert.match(state.createdTabs[0].url, /^chrome-extension:\/\/test-extension\/error\.html\?/);
  });
}

test("a missing native host opens an actionable extension error page", async () => {
  const { api, state } = apiWithNativeReply(new Error("Specified native messaging host not found"));
  const result = await dispatchContextClick(
    { menuItemId: MENU_ID, pageUrl: "https://example.test/watch?v=1" },
    {},
    api,
  );

  assert.deepEqual(result, { ok: false, code: "helper_unavailable" });
  assert.equal(state.messages.length, 1);
  assert.equal(state.createdTabs.length, 1);
  assert.match(state.createdTabs[0].url, /code=helper_unavailable/);
});

test("a missing player response is surfaced rather than reported as playback", async () => {
  const { api, state } = apiWithNativeReply({
    ok: false,
    code: "player_not_found",
    message: "OK Player was not found at the configured path.",
  });
  const result = await dispatchActionClick(
    { url: "https://www.instagram.com/reel/example/?utm_source=test#clip" },
    api,
  );

  assert.deepEqual(result, { ok: false, code: "player_not_found" });
  assert.equal(state.messages.length, 1);
  assert.equal(state.createdTabs.length, 1);
  assert.match(state.createdTabs[0].url, /code=player_not_found/);
});

test("one registered context-menu click produces one native request", async () => {
  const { api, state } = apiWithNativeReply();
  registerBrowserIntegration(api);
  state.installListener();
  assert.deepEqual(state.menu, {
    id: MENU_ID,
    title: "Open in OK Player",
    contexts: ["link", "video", "page", "action"],
  });

  state.contextListener(
    {
      menuItemId: MENU_ID,
      linkUrl: "https://facebook.com/watch/?v=101&ref=test#video",
    },
    {},
  );
  await new Promise((resolve) => setImmediate(resolve));
  assert.equal(state.messages.length, 1);
});
