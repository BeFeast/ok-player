# Open in OK Player browser integration

This directory contains the initial Linux integration for Chromium and Helium.
It adds **Open in OK Player** to link, video, page, and extension-action context
menus. Clicking the toolbar action opens the current page too.

The extension has a stable Chromium ID,
`mckhcemmkmogggicmpfkmpgekeagegnl`, derived from the public key in
`extension/manifest.json`. The installer binds the native host to that one ID.
The only extension permissions are `activeTab`, `contextMenus`, and
`nativeMessaging`; there are no host permissions or content scripts.

## What is sent

- A link click sends the original HTTP or HTTPS link, including its query,
  fragment, and Unicode text.
- A video click sends the page URL on supported YouTube, TikTok, Instagram,
  Facebook, X/Twitter, and `fb.watch` pages. A `blob:` video also uses its page
  URL. On another site, an ordinary HTTP or HTTPS video source is sent instead.
- A page-menu or toolbar-action click sends the current HTTP or HTTPS page.

Each click makes one native request. The Python host validates one bounded
UTF-8 JSON message and starts the configured OK Player executable with the URL
as its only argument. It does not use a shell. Child standard output and error
are kept away from the native-messaging stream. A successful response means
only that the launch request started; it does not claim playback succeeded.

The existing OK Player CLI and its single-instance handoff remain responsible
for opening the URL and recording it in player history. This integration does
not add a local server, downloader, browser-cookie access, remote-control
commands, or a global HTTP/HTTPS handler. The reserved `ok-player://` scheme is
unchanged.

## Install

Install OK Player first. The default launcher is the per-user GTK wrapper at
`USER_HOME/.local/bin/ok-player`; use `--player` when the installed executable
is elsewhere. Every path supplied to the installer must be absolute.

Preview a Helium install without changing files:

```sh
python3 browser-extension/install.py \
  --user-home /home/alice \
  --browser helium \
  --dry-run
```

Apply it and ask the Helium Linux wrapper to sideload the unpacked extension on
its next launch:

```sh
python3 browser-extension/install.py \
  --user-home /home/alice \
  --browser helium \
  --load-extension \
  --apply
```

Helium's default native-host directory is
`/home/alice/.config/net.imput.helium/NativeMessagingHosts`. The optional flag
is added to `/home/alice/.config/helium-browser-flags.conf`, which the Helium
Linux wrapper reads. Existing flag lines are retained. This command neither
starts nor restarts Helium; close and relaunch it yourself when appropriate.

For Chromium:

```sh
python3 browser-extension/install.py \
  --user-home /home/alice \
  --browser chromium \
  --apply
```

Chromium's default native-host directory is
`/home/alice/.config/chromium/NativeMessagingHosts`. Open
`chrome://extensions`, enable Developer mode, choose **Load unpacked**, and
select the installed directory printed by the installer:

```text
/home/alice/.local/share/ok-player/browser-extension/chromium/extension
```

The installer does not operate the browser UI. Installing a native manifest or
desktop entry alone does not install the extension.

Use `--browser-config-dir /absolute/path` for a non-default browser config
root, `--browser-flags-file /absolute/path` for a non-default Helium wrapper
flags file, and `--player /absolute/path/to/ok-player` for another installed
launcher. Only the Helium mode accepts `--load-extension`.

The installer records prior contents before replacing any managed file. It
refuses symbolic links, oversized backups, and later-modified managed files.
Uninstall restores pre-existing files. If the Helium flags file changed after
installation, uninstall removes only the exact line it added and leaves other
lines alone.

## Remove or roll back

Either command restores the files recorded by the last apply:

```sh
python3 browser-extension/install.py \
  --user-home /home/alice \
  --browser helium \
  --uninstall
```

```sh
python3 browser-extension/install.py \
  --user-home /home/alice \
  --browser helium \
  --rollback
```

Restart the browser yourself after removal. No GUI is launched by the
installer.

## Playback and privacy boundaries

OK Player receives the public page or media URL, not the browser's authenticated
session. Private, paywalled, age-gated, region-limited, or login-required media
may fail even when it plays in the browser. The extension and native host never
inspect browsing data or transfer cookies; the installer reads only the exact
native-host and optional flags files that it backs up or changes. Unsupported
schemes, option-looking values, control characters, malformed URLs, and
oversized requests are rejected before player launch. Missing helper and player
installations open an extension-owned page with a corrective action.

## Focused tests

No package installation is required:

```sh
node --test browser-extension/tests/bridge.test.mjs
PYTHONDONTWRITEBYTECODE=1 python3 -m unittest discover \
  -s browser-extension/tests -p 'test_*.py' -v
```
