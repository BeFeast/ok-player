#!/usr/bin/env python3
"""Install or remove the per-user OK Player Chromium native-messaging bridge."""

from __future__ import annotations

import argparse
import base64
from dataclasses import dataclass
import hashlib
import json
import os
from pathlib import Path
import stat
import tempfile

HOST_NAME = "org.ok_player.browser"
STATE_VERSION = 1
MAX_BACKUP_BYTES = 1024 * 1024
EXTENSION_FILES = (
    "manifest.json",
    "bridge.mjs",
    "background.js",
    "error.html",
    "error.js",
)
BROWSER_CONFIG_DEFAULTS = {
    "helium": Path(".config/net.imput.helium"),
    "chromium": Path(".config/chromium"),
}


class InstallError(Exception):
    pass


@dataclass(frozen=True)
class Target:
    path: Path
    content: bytes
    mode: int
    kind: str = "file"
    managed_line: str | None = None


def require_absolute(value: str, option: str) -> Path:
    path = Path(value)
    if not path.is_absolute():
        raise InstallError(f"{option} must be an absolute path: {value}")
    return path


def sha256(content: bytes) -> str:
    return hashlib.sha256(content).hexdigest()


def extension_id(manifest: dict[str, object]) -> str:
    key = manifest.get("key")
    if not isinstance(key, str):
        raise InstallError("The extension manifest has no stable public key.")
    try:
        digest = hashlib.sha256(base64.b64decode(key, validate=True)).hexdigest()[:32]
    except (ValueError, TypeError) as error:
        raise InstallError("The extension manifest public key is invalid.") from error
    return "".join(chr(ord("a") + int(nibble, 16)) for nibble in digest)


def json_bytes(value: object) -> bytes:
    return (json.dumps(value, indent=2, ensure_ascii=False) + "\n").encode("utf-8")


def read_small_regular_file(path: Path) -> tuple[bytes, int] | None:
    if path.is_symlink():
        raise InstallError(f"Refusing to replace symbolic link: {path}")
    if not path.exists():
        return None
    if not path.is_file():
        raise InstallError(f"Refusing to replace non-file path: {path}")
    size = path.stat().st_size
    if size > MAX_BACKUP_BYTES:
        raise InstallError(f"Refusing to back up file larger than {MAX_BACKUP_BYTES} bytes: {path}")
    return path.read_bytes(), stat.S_IMODE(path.stat().st_mode)


def atomic_write(path: Path, content: bytes, mode: int) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    temporary_name: str | None = None
    try:
        with tempfile.NamedTemporaryFile(
            dir=path.parent, prefix=f".{path.name}.", delete=False
        ) as stream:
            temporary_name = stream.name
            stream.write(content)
            stream.flush()
            os.fsync(stream.fileno())
        os.chmod(temporary_name, mode)
        os.replace(temporary_name, path)
        temporary_name = None
    finally:
        if temporary_name is not None:
            Path(temporary_name).unlink(missing_ok=True)


def append_flag(existing: bytes, flag: str) -> bytes:
    try:
        text = existing.decode("utf-8")
    except UnicodeDecodeError as error:
        raise InstallError("The Helium flags file is not valid UTF-8.") from error
    if any(line.rstrip("\r\n") == flag for line in text.splitlines(keepends=True)):
        return existing
    separator = "" if not text or text.endswith(("\n", "\r")) else "\n"
    return f"{text}{separator}{flag}\n".encode("utf-8")


def remove_one_flag(content: bytes, flag: str) -> bytes:
    try:
        lines = content.decode("utf-8").splitlines(keepends=True)
    except UnicodeDecodeError as error:
        raise InstallError("The Helium flags file is no longer valid UTF-8.") from error
    for index, line in enumerate(lines):
        if line.rstrip("\r\n") == flag:
            del lines[index]
            break
    return "".join(lines).encode("utf-8")


def source_root() -> Path:
    return Path(__file__).resolve().parent


def state_path(user_home: Path, browser: str) -> Path:
    return user_home / ".local/state/ok-player/browser-extension" / f"{browser}.json"


def browser_config_path(args, user_home: Path) -> Path:
    if args.browser_config_dir:
        return require_absolute(args.browser_config_dir, "--browser-config-dir")
    return user_home / BROWSER_CONFIG_DEFAULTS[args.browser]


def build_targets(args, user_home: Path) -> tuple[list[Target], Path, str]:
    root = source_root()
    extension_source = root / "extension"
    host_source = root / "host/ok_player_browser_host.py"
    try:
        manifest = json.loads((extension_source / "manifest.json").read_text(encoding="utf-8"))
    except (OSError, UnicodeDecodeError, json.JSONDecodeError) as error:
        raise InstallError("The bundled extension manifest could not be read.") from error
    if not isinstance(manifest, dict):
        raise InstallError("The bundled extension manifest is invalid.")
    public_id = extension_id(manifest)

    player = (
        require_absolute(args.player, "--player")
        if args.player
        else user_home / ".local/bin/ok-player"
    )
    if not player.is_file():
        raise InstallError(
            f"OK Player was not found at {player}. Pass --player with its absolute installed path."
        )
    if not os.access(player, os.X_OK):
        raise InstallError(
            f"OK Player is not executable at {player}. Fix its permissions or choose another --player path."
        )

    install_root = user_home / ".local/share/ok-player/browser-extension" / args.browser
    installed_extension = install_root / "extension"
    installed_host = user_home / ".local/lib/ok-player/browser-extension" / args.browser
    host_target = installed_host / "ok_player_browser_host.py"

    targets: list[Target] = []
    for name in EXTENSION_FILES:
        source = extension_source / name
        try:
            content = source.read_bytes()
        except OSError as error:
            raise InstallError(f"The bundled extension file is missing: {source}") from error
        targets.append(Target(installed_extension / name, content, 0o644))
    try:
        host_content = host_source.read_bytes()
    except OSError as error:
        raise InstallError(f"The bundled native host is missing: {host_source}") from error
    targets.append(Target(host_target, host_content, 0o755))
    targets.append(
        Target(
            installed_host / "host-config.json",
            json_bytes({"player_path": os.fspath(player)}),
            0o600,
        )
    )

    native_manifest = {
        "name": HOST_NAME,
        "description": "Open URLs in the installed OK Player",
        "path": os.fspath(host_target),
        "type": "stdio",
        "allowed_origins": [f"chrome-extension://{public_id}/"],
    }
    native_target = (
        browser_config_path(args, user_home)
        / "NativeMessagingHosts"
        / f"{HOST_NAME}.json"
    )
    targets.append(Target(native_target, json_bytes(native_manifest), 0o644))

    if args.load_extension:
        if args.browser != "helium":
            raise InstallError("--load-extension is supported only for the Helium Linux wrapper.")
        flags_path = (
            require_absolute(args.browser_flags_file, "--browser-flags-file")
            if args.browser_flags_file
            else user_home / ".config/helium-browser-flags.conf"
        )
        prior = read_small_regular_file(flags_path)
        prior_content, prior_mode = prior if prior is not None else (b"", 0o644)
        flag = f"--load-extension={installed_extension}"
        updated = append_flag(prior_content, flag)
        if updated != prior_content:
            targets.append(Target(flags_path, updated, prior_mode, "flag", flag))

    return targets, player, public_id


def record_targets(targets: list[Target]) -> list[dict[str, object]]:
    records: list[dict[str, object]] = []
    seen: set[Path] = set()
    for target in targets:
        if target.path in seen:
            raise InstallError(f"Install plan contains a duplicate target: {target.path}")
        seen.add(target.path)
        prior = read_small_regular_file(target.path)
        prior_content, prior_mode = prior if prior is not None else (None, None)
        records.append(
            {
                "path": os.fspath(target.path),
                "previous": (
                    base64.b64encode(prior_content).decode("ascii")
                    if prior_content is not None
                    else None
                ),
                "previous_mode": prior_mode,
                "installed_sha256": sha256(target.content),
                "kind": target.kind,
                "managed_line": target.managed_line,
            }
        )
    return records


def decode_previous(record: dict[str, object]) -> bytes | None:
    encoded = record.get("previous")
    if encoded is None:
        return None
    if not isinstance(encoded, str):
        raise InstallError("The install state contains an invalid backup.")
    try:
        return base64.b64decode(encoded, validate=True)
    except (ValueError, TypeError) as error:
        raise InstallError("The install state contains an invalid backup.") from error


def restore_target(record: dict[str, object]) -> None:
    raw_path = record.get("path")
    installed_hash = record.get("installed_sha256")
    if not isinstance(raw_path, str) or not Path(raw_path).is_absolute():
        raise InstallError("The install state contains an invalid target path.")
    if not isinstance(installed_hash, str) or len(installed_hash) != 64:
        raise InstallError("The install state contains an invalid installed checksum.")
    path = Path(raw_path)
    prior = decode_previous(record)
    prior_mode = record.get("previous_mode")
    if prior is not None and not isinstance(prior_mode, int):
        raise InstallError("The install state contains an invalid prior file mode.")
    current = read_small_regular_file(path)
    if current is None:
        if prior is not None:
            atomic_write(path, prior, prior_mode)
        return

    current_content, current_mode = current
    current_hash = sha256(current_content)
    if current_hash == installed_hash:
        if prior is None:
            path.unlink()
        else:
            atomic_write(path, prior, prior_mode)
        return
    if prior is not None and current_hash == sha256(prior):
        return

    if record.get("kind") == "flag":
        managed_line = record.get("managed_line")
        if not isinstance(managed_line, str):
            raise InstallError("The install state contains an invalid managed flag.")
        updated = remove_one_flag(current_content, managed_line)
        if updated != current_content:
            atomic_write(path, updated, current_mode)
        return

    raise InstallError(
        f"Refusing to overwrite a file changed after installation: {path}. Preserve or move it, then retry uninstall."
    )


def install(args, user_home: Path, dry_run: bool) -> int:
    state = state_path(user_home, args.browser)
    if state.is_symlink():
        raise InstallError(f"Refusing symbolic-link install state: {state}")
    if state.exists():
        raise InstallError(
            f"Browser integration is already recorded for {args.browser}. Run --uninstall before applying it again."
        )
    targets, player, public_id = build_targets(args, user_home)
    records = record_targets(targets)
    verb = "Would install" if dry_run else "Installing"
    print(f"{verb} extension {public_id} for {args.browser} with player {player}")
    for target in targets:
        print(f"  {target.path}")
    if dry_run:
        print("Dry run only; no files were changed and no browser was started.")
        return 0

    state_document = {
        "version": STATE_VERSION,
        "browser": args.browser,
        "extension_id": public_id,
        "targets": records,
    }
    atomic_write(state, json_bytes(state_document), 0o600)
    try:
        for target in targets:
            atomic_write(target.path, target.content, target.mode)
    except Exception as error:
        rollback_failed = False
        for record in reversed(records):
            try:
                restore_target(record)
            except (InstallError, OSError):
                rollback_failed = True
        if rollback_failed:
            raise InstallError(
                f"Installation failed and automatic rollback was incomplete. Run --rollback using the state at {state}."
            ) from error
        state.unlink(missing_ok=True)
        raise InstallError(f"Installation failed: {error}") from error
    print("Installed files. Restart the browser yourself to load them; no browser was started.")
    return 0


def uninstall(args, user_home: Path) -> int:
    state = state_path(user_home, args.browser)
    if state.is_symlink() or not state.is_file():
        raise InstallError(f"No recorded {args.browser} browser integration was found at {state}.")
    try:
        document = json.loads(state.read_text(encoding="utf-8"))
    except (OSError, UnicodeDecodeError, json.JSONDecodeError) as error:
        raise InstallError(f"The install state is unreadable: {state}") from error
    if (
        not isinstance(document, dict)
        or document.get("version") != STATE_VERSION
        or document.get("browser") != args.browser
        or not isinstance(document.get("targets"), list)
    ):
        raise InstallError(f"The install state is invalid: {state}")

    records = document["targets"]
    # Detect conflicts before removing any ordinary managed file.
    for record in records:
        if not isinstance(record, dict):
            raise InstallError("The install state contains an invalid target record.")
        if record.get("kind") == "flag":
            continue
        raw_path = record.get("path")
        installed_hash = record.get("installed_sha256")
        if (
            not isinstance(raw_path, str)
            or not Path(raw_path).is_absolute()
            or not isinstance(installed_hash, str)
        ):
            raise InstallError("The install state contains an invalid target record.")
        current = read_small_regular_file(Path(raw_path))
        prior = decode_previous(record)
        if current is None:
            continue
        current_hash = sha256(current[0])
        if current_hash == installed_hash or (prior is not None and current_hash == sha256(prior)):
            continue
        raise InstallError(
            f"Refusing to overwrite a file changed after installation: {raw_path}. Preserve or move it, then retry uninstall."
        )

    for record in reversed(records):
        restore_target(record)
    state.unlink()
    print(f"Removed the recorded {args.browser} browser integration; unrelated files and flags were preserved.")
    return 0


def parser() -> argparse.ArgumentParser:
    result = argparse.ArgumentParser(
        description="Install the per-user OK Player browser extension and native host."
    )
    result.add_argument(
        "--user-home", required=True, help="Absolute home directory of the target user"
    )
    result.add_argument("--browser", required=True, choices=sorted(BROWSER_CONFIG_DEFAULTS))
    result.add_argument(
        "--browser-config-dir",
        help="Absolute browser profile configuration root; defaults from --user-home and --browser",
    )
    result.add_argument(
        "--browser-flags-file",
        help="Absolute Helium wrapper flags file; defaults to USER_HOME/.config/helium-browser-flags.conf",
    )
    result.add_argument(
        "--player",
        help="Absolute installed OK Player launcher; defaults to USER_HOME/.local/bin/ok-player",
    )
    result.add_argument(
        "--load-extension",
        action="store_true",
        help="Add the unpacked extension path to the Helium wrapper flags for its next launch",
    )
    actions = result.add_mutually_exclusive_group(required=True)
    actions.add_argument("--dry-run", action="store_true", help="Print the plan without changing files")
    actions.add_argument("--apply", action="store_true", help="Install the integration")
    actions.add_argument("--rollback", action="store_true", help="Restore files recorded by the last apply")
    actions.add_argument("--uninstall", action="store_true", help="Alias for --rollback")
    return result


def main(argv: list[str] | None = None) -> int:
    args = parser().parse_args(argv)
    try:
        user_home = require_absolute(args.user_home, "--user-home")
        if args.rollback or args.uninstall:
            return uninstall(args, user_home)
        return install(args, user_home, args.dry_run)
    except (InstallError, OSError) as error:
        parser().error(str(error))
    return 2


if __name__ == "__main__":
    raise SystemExit(main())
