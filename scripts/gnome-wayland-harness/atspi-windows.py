#!/usr/bin/env python3
"""List the toplevel windows the accessibility bus can see, with the app that owns each.

The chooser rows involve another process's UI, and this is how the harness can say which
process actually put the dialog on screen instead of assuming the app drew it itself. It
reads names and roles only: no contents, no user data.
"""

from __future__ import annotations

import json
import sys

import gi

gi.require_version("Atspi", "2.0")
from gi.repository import Atspi  # noqa: E402


def main() -> int:
    Atspi.init()
    desktop = Atspi.get_desktop(0)
    windows = []
    for index in range(desktop.get_child_count()):
        application = desktop.get_child_at_index(index)
        if application is None:
            continue
        try:
            application_name = application.get_name()
            for child_index in range(application.get_child_count()):
                window = application.get_child_at_index(child_index)
                if window is None:
                    continue
                states = window.get_state_set()
                windows.append(
                    {
                        "application": application_name,
                        "window": window.get_name(),
                        "role": window.get_role_name(),
                        "active": states.contains(Atspi.StateType.ACTIVE),
                        "showing": states.contains(Atspi.StateType.SHOWING),
                    }
                )
        except Exception:  # a client may vanish mid-walk; it simply is not listed
            continue
    json.dump(windows, sys.stdout, indent=2, sort_keys=True)
    sys.stdout.write("\n")
    return 0


if __name__ == "__main__":
    sys.exit(main())
