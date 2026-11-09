# macOS failure card and toast (issue #887)

The app rendered its own window in light and dark during the GitHub-hosted smoke of
commit a76e8da1; the OpenGL video area is not part of that render.

- `status-light.png`, `status-dark.png`: the failure card the session reports after
  the smoke opens a missing file and retries it (the core's "Playback failed" card
  with Retry, Open another and Copy details), over the transport controls.
- `window-light.png`, `window-dark.png`: the fixture playing, with the toast the
  session's notice for a volume nudge shows near the top of the window.
