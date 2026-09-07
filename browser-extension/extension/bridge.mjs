export const MENU_ID = "open-in-ok-player";
export const NATIVE_HOST = "org.ok_player.browser";
export const MAX_URL_BYTES = 32 * 1024;

const SUPPORTED_PAGE_HOSTS = [
  "youtube.com",
  "youtu.be",
  "tiktok.com",
  "instagram.com",
  "facebook.com",
  "fb.watch",
  "x.com",
  "twitter.com",
];

export class UrlInputError extends Error {
  constructor(code, message) {
    super(message);
    this.name = "UrlInputError";
    this.code = code;
  }
}

function containsControlCharacter(value) {
  return Array.from(value).some((character) => {
    const codePoint = character.codePointAt(0);
    return /\p{Cc}/u.test(character) || (codePoint >= 0xd800 && codePoint <= 0xdfff);
  });
}

export function validateHttpUrl(value) {
  if (typeof value !== "string" || value.length === 0) {
    throw new UrlInputError("missing_url", "This menu item did not provide a URL.");
  }
  if (value !== value.trim()) {
    throw new UrlInputError("unsafe_url", "URLs with surrounding whitespace are not accepted.");
  }
  if (value.startsWith("-")) {
    throw new UrlInputError("unsafe_url", "Option-looking input is not accepted.");
  }
  if (containsControlCharacter(value)) {
    throw new UrlInputError("unsafe_url", "URLs containing control characters are not accepted.");
  }
  if (new TextEncoder().encode(value).length > MAX_URL_BYTES) {
    throw new UrlInputError("unsafe_url", "This URL is too long to send safely.");
  }

  let parsed;
  try {
    parsed = new URL(value);
  } catch {
    throw new UrlInputError("unsupported_url", "Only complete HTTP and HTTPS URLs are supported.");
  }
  if ((parsed.protocol !== "http:" && parsed.protocol !== "https:") || !parsed.hostname) {
    throw new UrlInputError("unsupported_url", "Only complete HTTP and HTTPS URLs are supported.");
  }
  return value;
}

export function isSupportedVideoPage(value) {
  try {
    const parsed = new URL(validateHttpUrl(value));
    const hostname = parsed.hostname.toLowerCase().replace(/\.$/, "");
    return SUPPORTED_PAGE_HOSTS.some(
      (host) => hostname === host || hostname.endsWith(`.${host}`),
    );
  } catch {
    return false;
  }
}

function currentPageUrl(info, tab) {
  return info.pageUrl || tab?.url || "";
}

export function selectUrl(info = {}, tab = {}) {
  if (typeof info.linkUrl === "string" && info.linkUrl.length > 0) {
    return validateHttpUrl(info.linkUrl);
  }

  const pageUrl = currentPageUrl(info, tab);
  const isVideo = info.mediaType === "video" || typeof info.srcUrl === "string";
  if (isVideo) {
    const sourceUrl = info.srcUrl || "";
    if (sourceUrl.toLowerCase().startsWith("blob:") || isSupportedVideoPage(pageUrl)) {
      try {
        return validateHttpUrl(pageUrl);
      } catch {
        throw new UrlInputError(
          "unsupported_video",
          "This temporary video source needs an HTTP or HTTPS page URL.",
        );
      }
    }
    return validateHttpUrl(sourceUrl);
  }

  return validateHttpUrl(pageUrl);
}

function errorPageUrl(api, code, message) {
  const query = new URLSearchParams({ code, message });
  return `${api.runtime.getURL("error.html")}?${query.toString()}`;
}

export function showError(api, code, message) {
  api.tabs.create({ url: errorPageUrl(api, code, message) });
}

export function openUrlInPlayer(value, api = globalThis.chrome) {
  let url;
  try {
    url = validateHttpUrl(value);
  } catch (error) {
    const code = error instanceof UrlInputError ? error.code : "invalid_request";
    showError(api, code, error.message);
    return Promise.resolve({ ok: false, code });
  }

  return new Promise((resolve) => {
    api.runtime.sendNativeMessage(NATIVE_HOST, { url }, (response) => {
      const transportError = api.runtime.lastError;
      if (transportError) {
        showError(
          api,
          "helper_unavailable",
          "The OK Player browser helper is not installed or could not be started. Run the browser integration installer, then try again.",
        );
        resolve({ ok: false, code: "helper_unavailable" });
        return;
      }

      if (!response || response.ok !== true) {
        const code = typeof response?.code === "string" ? response.code : "helper_error";
        const message =
          typeof response?.message === "string"
            ? response.message
            : "The OK Player browser helper returned an invalid response. Reinstall the browser integration.";
        showError(api, code, message);
        resolve({ ok: false, code });
        return;
      }

      resolve(response);
    });
  });
}

export function dispatchContextClick(info, tab, api = globalThis.chrome) {
  if (info.menuItemId !== MENU_ID) {
    return Promise.resolve({ ok: false, code: "unrelated_menu" });
  }
  try {
    return openUrlInPlayer(selectUrl(info, tab), api);
  } catch (error) {
    const code = error instanceof UrlInputError ? error.code : "invalid_request";
    showError(api, code, error.message);
    return Promise.resolve({ ok: false, code });
  }
}

export function dispatchActionClick(tab, api = globalThis.chrome) {
  try {
    return openUrlInPlayer(validateHttpUrl(tab?.url || ""), api);
  } catch (error) {
    const code = error instanceof UrlInputError ? error.code : "invalid_request";
    showError(api, code, error.message);
    return Promise.resolve({ ok: false, code });
  }
}

export function registerBrowserIntegration(api = globalThis.chrome) {
  api.runtime.onInstalled.addListener(() => {
    api.contextMenus.removeAll(() => {
      void api.runtime.lastError;
      api.contextMenus.create({
        id: MENU_ID,
        title: "Open in OK Player",
        contexts: ["link", "video", "page", "action"],
      });
    });
  });
  api.contextMenus.onClicked.addListener((info, tab) => {
    void dispatchContextClick(info, tab, api);
  });
  api.action.onClicked.addListener((tab) => {
    void dispatchActionClick(tab, api);
  });
}
