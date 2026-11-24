# Linux AppImage delivery

OK Player ships a single x86_64 AppImage. The builder uses the Debian 13 ABI baseline,
the pinned bundled libmpv runtime, and the existing AppImage packager. Install GTK4,
libadwaita and the graphics drivers from your distribution. The launcher uses FUSE when
available and falls back to extraction when the FUSE device is absent.

## Install and update

Clone the canonical Forgejo repository and run `./scripts/install-linux-appimage.sh beta`.
The launcher is `~/.local/bin/ok-player`. It forwards files and URI arguments to the managed
AppImage under `$XDG_DATA_HOME/ok-player` (default `~/.local/share/ok-player`). The installer
registers the application and `ok-player://` desktop handler for the user.

**Settings → Updates** shows the selected channel, running build, last check, and
available build. Automatic checks run at launch, every 28 minutes, and on activation
with a five-minute throttle. **Install & Relaunch** verifies and stages the package
beside the managed AppImage. It waits for the player to exit, advances the current
link, then checks that the expected build reports startup. An unconfirmed startup
restores and reopens the previous build.

For a manual update with the player closed, run `ok-player --update beta` or
`ok-player --update stable`. Both routes share the install lock and verify the
selected manifest's OpenPGP signature, provenance, AppImage signature, size, and
SHA-256. The manifest signature is stored at a key derived from the manifest's
SHA-256 so a publication interruption leaves the previous pointer verifiable.
Only newer builds are installed. Settings and history carry forward across builds.

The managed `OK-Player.AppImage` link identifies the installed build. For a manual
rollback, close the player and point the link at the previous executable under
`builds/<number>/`.

## Release workflow

Dispatch **Linux AppImage** on canonical Forgejo `main`. `beta` builds the exact first-parent
`main` SHA and publishes it after a headless playback check. `archive` runs the same checks and
publishes the immutable files for a pinned revision. Specify the archived build number with
`stable` to promote its verified bytes. The build number is the commit's first-parent height,
so an archived revision keeps the same version across retries and channel promotion.

The build job hands off its AppImage, source SHA, SHA-256 and playback evidence to a separate
publisher job on the protected branch. Configure `R2_ENDPOINT`, `R2_ACCESS_KEY_ID`,
`R2_SECRET_ACCESS_KEY`, `OKP_GPG_PRIVATE_KEY` and `OKP_GPG_PASSPHRASE` for that job. The publisher
uploads immutable artifacts to `https://updates.befeast.com/ok-player/linux/builds/<number>/`.
The signed channel descriptions are at
[Beta](https://updates.befeast.com/ok-player/linux/beta/latest.json) and
[Stable](https://updates.befeast.com/ok-player/linux/stable/latest.json). Each description names
the exact versioned AppImage and its source, size and SHA-256.

## Historical packages

The Fedora RPM/COPR, Flatpak, GitHub Linux Release and Linux Candidate workflows are retired.
The old GNOME Wayland acceptance workflow is archived with them because it consumes GitHub
package runs. Existing GitHub Pages and APT feeds still serve historical installations. The
previous packaging scripts, workflow definitions and acceptance records remain in the tree.
