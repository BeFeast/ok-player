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

Run `ok-player --update beta` or `ok-player --update stable` when the player is closed.
The launcher and updater share a lock. This keeps a running image in place and makes an update
available on the next launch. The updater verifies the selected channel manifest's OpenPGP
signature, the immutable provenance it names, the AppImage signature, and the payload size and
SHA-256. The updater uses an immutable signature addressed by the manifest's SHA-256. This keeps
the current manifest verifiable if publication stops before advancing the channel. The signing
authority is the checked-in OK Player Linux archive key. The updater accepts
only newer builds and retains previous build directories for rollback. Settings and history use
the same per-user locations across builds.

The managed `OK-Player.AppImage` link identifies the installed build. To roll back, close the
player and point it at the previous executable in `builds/<number>/` under the same directory.

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
