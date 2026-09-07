#!/usr/bin/env python3
"""Bounded native-messaging bridge from Chromium to the OK Player CLI."""

from __future__ import annotations

import json
import os
from pathlib import Path
import struct
import subprocess
import sys
import unicodedata
from urllib.parse import urlsplit

MAX_MESSAGE_BYTES = 64 * 1024
MAX_URL_BYTES = 32 * 1024
MAX_CONFIG_BYTES = 16 * 1024


class RequestError(Exception):
    def __init__(self, code: str, message: str):
        super().__init__(message)
        self.code = code
        self.message = message


def _contains_control(value: str) -> bool:
    return any(unicodedata.category(character) in {"Cc", "Cs"} for character in value)


def validate_http_url(value: object) -> str:
    if not isinstance(value, str) or not value:
        raise RequestError("missing_url", "The browser request did not contain a URL.")
    if value != value.strip():
        raise RequestError("unsafe_url", "URLs with surrounding whitespace are not accepted.")
    if value.startswith("-"):
        raise RequestError("unsafe_url", "Option-looking input is not accepted.")
    if _contains_control(value):
        raise RequestError("unsafe_url", "URLs containing control characters are not accepted.")
    try:
        encoded_length = len(value.encode("utf-8"))
    except UnicodeEncodeError as error:
        raise RequestError("unsafe_url", "The URL contains invalid Unicode text.") from error
    if encoded_length > MAX_URL_BYTES:
        raise RequestError("unsafe_url", "This URL is too long to send safely.")

    try:
        parsed = urlsplit(value)
        # Accessing port makes urllib reject malformed bracket/port forms too.
        _ = parsed.port
    except ValueError as error:
        raise RequestError(
            "unsupported_url", "Only complete HTTP and HTTPS URLs are supported."
        ) from error
    if parsed.scheme.lower() not in {"http", "https"} or not parsed.hostname:
        raise RequestError(
            "unsupported_url", "Only complete HTTP and HTTPS URLs are supported."
        )
    return value


def read_exact(stream, size: int) -> bytes:
    chunks: list[bytes] = []
    remaining = size
    while remaining:
        chunk = stream.read(remaining)
        if not chunk:
            raise RequestError("invalid_request", "The browser request ended unexpectedly.")
        chunks.append(chunk)
        remaining -= len(chunk)
    return b"".join(chunks)


def read_native_message(stream) -> object:
    header = read_exact(stream, 4)
    (size,) = struct.unpack("<I", header)
    if size == 0:
        raise RequestError("invalid_request", "The browser request was empty.")
    if size > MAX_MESSAGE_BYTES:
        raise RequestError("invalid_request", "The browser request was too large.")
    payload = read_exact(stream, size)
    try:
        return json.loads(payload.decode("utf-8"))
    except (UnicodeDecodeError, json.JSONDecodeError) as error:
        raise RequestError("invalid_request", "The browser request was not valid UTF-8 JSON.") from error


def write_native_message(stream, response: dict[str, object]) -> None:
    payload = json.dumps(
        response, ensure_ascii=False, separators=(",", ":")
    ).encode("utf-8")
    stream.write(struct.pack("<I", len(payload)))
    stream.write(payload)
    stream.flush()


def load_player_path(config_path: Path) -> Path:
    try:
        raw = config_path.read_bytes()
    except OSError as error:
        raise RequestError(
            "host_not_configured",
            "The OK Player browser helper is not configured. Reinstall the browser integration.",
        ) from error
    if len(raw) > MAX_CONFIG_BYTES:
        raise RequestError(
            "host_not_configured",
            "The OK Player browser helper configuration is invalid. Reinstall the browser integration.",
        )
    try:
        config = json.loads(raw.decode("utf-8"))
    except (UnicodeDecodeError, json.JSONDecodeError) as error:
        raise RequestError(
            "host_not_configured",
            "The OK Player browser helper configuration is invalid. Reinstall the browser integration.",
        ) from error
    player = config.get("player_path") if isinstance(config, dict) else None
    if (
        not isinstance(player, str)
        or not player
        or _contains_control(player)
        or not Path(player).is_absolute()
    ):
        raise RequestError(
            "host_not_configured",
            "The OK Player browser helper has no valid absolute player path. Reinstall the browser integration.",
        )
    return Path(player)


def launch_request(message: object, config_path: Path) -> dict[str, object]:
    if not isinstance(message, dict) or set(message) != {"url"}:
        raise RequestError("invalid_request", "The browser request has an unsupported shape.")
    url = validate_http_url(message["url"])
    player = load_player_path(config_path)
    if not player.is_file():
        raise RequestError(
            "player_not_found",
            "OK Player was not found at the configured path. Reinstall the browser integration with --player set to the installed launcher.",
        )
    if not os.access(player, os.X_OK):
        raise RequestError(
            "player_not_executable",
            "The configured OK Player launcher is not executable. Fix its permissions or reinstall the browser integration.",
        )

    try:
        subprocess.Popen(
            [os.fspath(player), url],
            shell=False,
            stdin=subprocess.DEVNULL,
            stdout=subprocess.DEVNULL,
            stderr=subprocess.DEVNULL,
            close_fds=True,
            start_new_session=True,
        )
    except (OSError, ValueError) as error:
        raise RequestError(
            "launch_failed",
            "The OK Player launch request could not be started. Check the configured launcher and reinstall the browser integration if its path changed.",
        ) from error

    return {
        "ok": True,
        "status": "launch_requested",
        "message": "The OK Player launch request started; playback has not been confirmed.",
    }


def run(stdin, stdout, config_path: Path) -> int:
    try:
        request = read_native_message(stdin)
        response = launch_request(request, config_path)
    except RequestError as error:
        response = {"ok": False, "code": error.code, "message": error.message}
    except Exception:
        response = {
            "ok": False,
            "code": "helper_error",
            "message": "The OK Player browser helper failed unexpectedly. Reinstall it and try again.",
        }
    write_native_message(stdout, response)
    return 0


def main() -> int:
    config_path = Path(__file__).with_name("host-config.json")
    return run(sys.stdin.buffer, sys.stdout.buffer, config_path)


if __name__ == "__main__":
    raise SystemExit(main())
