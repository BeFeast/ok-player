# Linux drag and window-fit regression smokes

The non-publishing regression suite combines the real GTK/libmpv X11 smoke for
thresholded movement from non-OSC player surfaces with the three-consecutive-run
window-fit series:

```bash
CC=/usr/bin/cc cargo build --manifest-path rust/Cargo.toml -p okp-linux-gtk
OKP_LINUX_REGRESSION_SOURCE_SHA="$(git rev-parse HEAD)" \
OKP_LINUX_REGRESSION_RUNNER_LABEL=local-xvfb \
  ./scripts/run-linux-regression-smokes.sh \
  ./rust/target/debug/okp-linux-gtk \
  artifacts/linux-regression-smokes
```

The suite creates a fresh Xvfb, Xfwm, D-Bus, and XDG environment through the
underlying smokes. It fails if the process dies during video- or idle-surface
drag, after compositor cancellation, or during the recovery drag. The fit gate
then requires three complete clean sessions whose logged fitted windows stay
inside one selected monitor workarea. `suite-evidence.txt` binds both gate
records by SHA-256 and records the exact source SHA and a caller-supplied logical
runner label.

The existing Rust GitHub Actions workflow runs `cargo test --workspace`, which
executes the suite's orchestration contract with deterministic fake gates. Those
tests require drag-before-fit ordering, exact evidence validation, source and
runner metadata, and fail-fast behavior without needing a graphical runner.
The real GTK/libmpv command remains a night/QA gate because it deliberately
treats any product lifecycle crash as a failure instead of retrying it.

## Night GUI suite invocation

Night orchestration must obtain its whitespace-separated logical host list from
`OKP_LINUX_NIGHT_GUI_HOSTS`; it must not embed a workstation name in repository
scripts. On each leased host, the driver invokes the command above locally and
passes that logical entry as `OKP_LINUX_REGRESSION_RUNNER_LABEL`. The regression
wrapper itself performs no SSH, packaging, publishing, installation, or release
operation, so any headless host with the documented X11 dependencies can run it
without an operator seat. The night driver retains the complete output
directory and console log for each logical host.

This evidence is intentionally limited to deterministic X11/Xvfb behavior. It
does not prove live GNOME Wayland compositor behavior, portals, drag-and-drop,
clipboard or focus integration, or physical dual-head hardware acceptance.
