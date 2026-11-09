# macOS Up Next panel (issue #884)

The app rendered its own window in light (`window-light.png`) and dark (`window-dark.png`) during the GitHub-hosted smoke of commit 46dc67c8: the fixture plays from a folder holding two copies of it, and the Up Next panel lists both under the Now Playing card. The OpenGL video area is not part of that render.
