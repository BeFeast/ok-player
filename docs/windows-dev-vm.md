# Windows development VM

Most OK Player Windows-shell development runs in a **persistent Windows 11 VM**. This document describes
the reproducible bootstrap for that VM, the build/test/package loop it supports, and — importantly — the
line between what a VM can verify and what still needs real hardware.

The goal is a machine that is *provisioned from a script and a version manifest*, not a hand-configured box
whose state nobody can reproduce. Two files own that state:

- [`scripts/windows-dev-versions.json`](../scripts/windows-dev-versions.json) — the single source of truth
  for the system-provisioned toolchain (versions, winget IDs, VS workloads, the VM envelope). In-repo pinned
  dependencies (the .NET target framework, the Windows SDK build, the Windows App SDK / Windows SDK
  BuildTools NuGet versions) are **not** duplicated here; they live in the csproj files and
  `Directory.Packages.props` and are read from there, so nothing can drift.
- [`scripts/bootstrap-windows-dev-vm.ps1`](../scripts/bootstrap-windows-dev-vm.ps1) — the idempotent
  installer that reads the manifest and converges the VM to that state.

## VM envelope (development baseline)

| Resource | Baseline |
| --- | --- |
| vCPU | 8 |
| RAM | 16 GiB |
| System SSD | 160 GiB |

This is a **development** baseline for a comfortable WinUI + Rust build/test loop — it is **not** a runtime
requirement for OK Player. The shipped player targets ordinary Windows 11 hardware (see
`TargetPlatformMinVersion` in `src/OkPlayer.App/OkPlayer.App.csproj`). A smaller VM still builds; it is just
slower, and a full self-contained publish plus the native binaries want the disk headroom.

The guest OS is **Windows 11** (build 22621 or newer). The Windows 11 **SDK** the shell compiles against
(build 26100) is provisioned by the Visual Studio workload, independent of the guest OS build.

## Bootstrap

From an elevated PowerShell in a clean VM, with the public repository cloned:

```powershell
# First run may use in-box Windows PowerShell 5.1; the script installs PowerShell 7 for later runs.
powershell -ExecutionPolicy Bypass -File .\scripts\bootstrap-windows-dev-vm.ps1
```

It installs and verifies, via winget (App Installer, in-box on Windows 11):

- **Visual Studio 2026 Build Tools** (or Community) with the *Managed Desktop Build Tools* + *Visual C++
  Build Tools* workloads and the *Windows 11 SDK 26100* component.
- **.NET 9 SDK**, matching the `net9.0` / `net9.0-windows` target frameworks and the `setup-dotnet` pin in
  CI.
- **Rust** via `rustup`: the `stable` toolchain and the `x86_64-pc-windows-msvc` target (MSVC, not GNU, so
  the future `okp-ffi` C ABI links against the same runtime as the shell).
- **Git**, **PowerShell 7**, and — for building libmpv/ffmpeg from source only — **CMake** and **Ninja**.
- **7-Zip**, then **libmpv** and **ffmpeg** native binaries via [`scripts/fetch-natives.ps1`](../scripts/fetch-natives.ps1)
  (skip with `-SkipNatives`).

The bootstrap is **safe to re-run**: every step checks for the tool first and installs only what is missing,
so a fresh VM, a half-provisioned VM, and an already-complete VM all converge to the same verified state.
After a first-time Visual Studio or Rust install a new shell (or reboot) may be needed before every tool is
on `PATH`; re-running finishes the convergence. When it finishes it writes the environment report (below).

### Idempotency and verification without installing

```powershell
# Verify a snapshot's toolchain WITHOUT changing it. Non-zero exit = baseline not met.
pwsh .\scripts\bootstrap-windows-dev-vm.ps1 -CheckOnly
```

## Environment report

[`scripts/report-windows-env.ps1`](../scripts/report-windows-env.ps1) captures the versions that define the
build surface and checks them against the manifest baseline:

```powershell
pwsh .\scripts\report-windows-env.ps1 -OutFile artifacts\windows-env-report.json
```

The JSON report records **OS** (caption + build), **Visual Studio / MSVC** (product, version, required
workloads present), **Windows SDK** (app target framework + Windows SDK BuildTools package), **Windows App
SDK / WinUI**, **Rust** (rustc/cargo, active toolchain, installed targets), **.NET SDK**, **Git**, **CMake**,
**Ninja**, **PowerShell**, and the resolved **libmpv** native product version. WinUI / Windows App SDK and the
Windows SDK BuildTools versions are read from `Directory.Packages.props`; the target framework from the app
csproj — the report cannot disagree with what the build restores. `overall: ok` means the guest OS build meets
the `22621` floor **and** every required tool is present and at or above its baseline (CMake/Ninja are optional
and never fail the report), so a VM on an unsupported older Windows build fails the gate even when its tools are
current.

## Build / test / package loop

All commands run from the repository root inside the VM.

```powershell
# Native engine binaries (once per checkout; idempotent).
pwsh .\scripts\fetch-natives.ps1

# Build the whole solution.
dotnet build OkPlayer.sln -c Debug

# Engine-agnostic Core unit tests (headless — VM-valid).
dotnet test tests\OkPlayer.Tests\OkPlayer.Tests.csproj -c Release

# Real-libmpv integration tests incl. the render-thread guard (Debug so the guard is compiled in).
dotnet test tests\OkPlayer.IntegrationTests\OkPlayer.IntegrationTests.csproj -c Debug

# Clean, correct Release build of the shell (guards against a Frankenbuild), optionally launched.
pwsh .\scripts\run-clean.ps1 -AllowBranch -NoLaunch

# Development package: the Velopack Setup.exe + portable zip (local pack, no upload).
pwsh .\installer\build-velopack.ps1

# Updater test: a local pack produces releases.win.json + full/delta .nupkg; verifying the feed
# derivation itself is a local, network-free step. Do NOT run build-velopack.ps1 -Publish from a VM —
# publishing mutates the live GitHub release/feed and is an operator action, not a dev-loop step.
```

### Rust workspace

The Rust workspace under `rust/` builds on the same VM (it is the Linux shell's core, but `okp-core` and the
tests are portable). Standard gates:

```powershell
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

## VM-valid gates vs. real-hardware gates

A persistent VM verifies most of the shell, but a normal VM's paravirtual GPU, audio, and display stack
**cannot** accept the hardware-bound rows. Keep the two classes of evidence separate and never mark a
hardware row `PASS` from inside a VM.

### VM-valid (accept in the VM)

- Core unit tests (chapter math, history/settings sidecars, schema, all engine-agnostic logic).
- Real-libmpv integration tests: open-time deadlock guard, render-thread guard contract, property/event
  plumbing, seek/argument dispatch.
- Build, clean-build, and development-package (`build-velopack.ps1` local pack) production.
- The updater feed **derivation** (local `releases.win.json` + `.nupkg` generation).
- The environment report and toolchain baseline check.

### Real-hardware only (do NOT accept in a normal VM)

- **D3D11 hardware video decode** and the render-API interop under a real GPU.
- **HDR** output / tone-mapping and 10-bit presentation.
- **4K60** sustained presentation and drop-free playback.
- **Multi-monitor DPI** transitions and per-monitor scaling.
- **Physical audio-device** acceptance (exclusive/shared mode, device switching, latency).

These are accepted on the real-hardware verification machine, not in the VM. State the VM limit explicitly in
any acceptance note; a VM screenshot proves deterministic composition, not GPU/HDR/audio behavior.

## Parked physical Windows checkout — policy unchanged

The dedicated physical Windows verification checkout used for the real-hardware gates above stays **parked and
manually owned**. This VM bootstrap provisions the machine it runs on and clones/mutates nothing outside this
repository; it adds **no** automation that pulls, updates, or otherwise mutates that verification checkout.
Keeping the physical checkout under manual control is deliberate — its whole value is being a stable,
operator-known reference for hardware acceptance.

## Clean snapshot

Once the report shows `overall: ok`, take a VM snapshot as the reproducible baseline:

1. Run `pwsh .\scripts\bootstrap-windows-dev-vm.ps1` (or `-CheckOnly` on an existing VM) until the report is
   `overall: ok`.
2. Save `artifacts\windows-env-report.json` with the snapshot as its provenance record.
3. Shut the guest down cleanly and take the snapshot (e.g. *clean-dev-baseline*).
4. To validate a restored snapshot, run `-CheckOnly` — it must exit `0` without installing anything.

Re-running the bootstrap on a restored snapshot converges to the same state, so the snapshot and the script
stay interchangeable ways to reach the baseline.
