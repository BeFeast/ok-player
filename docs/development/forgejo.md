# Forgejo development and GitHub distribution

The canonical repository and issue tracker are [Forgejo](https://git.oklabs.uk/BeFeast/ok-player).
[GitHub](https://github.com/BeFeast/ok-player) remains the public downstream and existing distribution surface.

`.forgejo/workflows/rust.yml` runs the complete existing Rust workspace gate,
including formatting, Clippy, workspace tests, packaging policy tests and virtual
display smoke tests. Pull request declarations are read live from Forgejo so
reruns cannot approve a stale title or acceptance block. The bootstrap branch
push trigger exists to exercise Actions before the workflow reaches `main`;
it does not satisfy the PR declaration gate.

Windows integration and the separate Debian APT provisioning gate still run on
GitHub. They require Windows and an isolated container runtime respectively;
the default Forgejo container runner provides neither. Their GitHub workflows,
release tags, Pages/APT publisher and update URLs remain unchanged. Passing
Forgejo Rust alone does not replace those existing merge requirements. A
migration PR needs matching-head GitHub checks and the configured review gate
before merge; do not relax required checks to finish the cutover.

Code flows Forgejo to GitHub. External GitHub issues and pull requests are intake
sources; they must retain their source URL when brought into the canonical
tracker. Source mirroring does not mirror release asset bytes or comments, and
must not be reported as a verified release/intake bridge.

Releases remain operator-triggered under the existing Linux/Windows release
policy. Never create a release tag to test the migration.
