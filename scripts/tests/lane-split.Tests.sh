#!/usr/bin/env bash
# The two-lane split, asserted against the parsed workflows rather than their text.
#
# A pull request is gated by the three required checks only. The deep lanes - Fedora RPM,
# Windows Installer, Offline Flatpak - run on pushes to main, nightly and on demand, so a
# user-visible fix reaches a candidate in minutes instead of waiting on lanes it cannot break.
#
# Parsed, not grepped: a comment mentioning a trigger, or a duplicated string, must not be able
# to make this pass or fail. The previous form of this check counted occurrences of "- 'src/**'"
# in the file text and broke the moment a trigger was removed.
set -euo pipefail

cd "$(dirname "${BASH_SOURCE[0]}")/../.."

pass_count=0
fail_count=0
pass() { printf 'PASS %s\n' "$1"; pass_count=$((pass_count + 1)); }
fail() { printf 'FAIL %s: %s\n' "$1" "$2"; fail_count=$((fail_count + 1)); }

read_policy() {
  python3 - "$1" <<'PY'
import sys, yaml
d = yaml.safe_load(open(sys.argv[1]))
on = d.get(True) or d.get("on") or {}
triggers = sorted(on.keys()) if isinstance(on, dict) else [str(on)]
push = (on.get("push") or {}) if isinstance(on, dict) else {}
paths = push.get("paths") or []
print("triggers=" + ",".join(triggers))
print("push_paths=" + "\x1f".join(paths))
PY
}

DEEP_LANES=(
  .github/workflows/rpm.yml
  .github/workflows/windows-package.yml
  .github/workflows/flatpak.yml
)

for lane in "${DEEP_LANES[@]}"; do
  policy="$(read_policy "$lane")"
  triggers="${policy%%$'\n'*}"
  triggers="${triggers#triggers=}"
  case ",$triggers," in
    *,pull_request,*)
      fail "$lane is a deep lane" "it still subscribes to pull_request ($triggers)" ;;
    *)
      pass "$lane does not gate a pull request" ;;
  esac
  case ",$triggers," in
    *,push,*) pass "$lane still runs on main" ;;
    *) fail "$lane must keep its push trigger" "triggers: $triggers" ;;
  esac
  case ",$triggers," in
    *,schedule,*) pass "$lane runs nightly" ;;
    *) fail "$lane must run nightly" "triggers: $triggers" ;;
  esac
done

# The Windows lane builds the installer, so its push filter must still reach everything the
# installer contains - the assertion the occurrence count used to stand in for.
policy="$(read_policy .github/workflows/windows-package.yml)"
push_paths="${policy##*$'\n'}"
push_paths="${push_paths#push_paths=}"
for required in 'src/**' 'LICENSE' 'THIRD-PARTY-NOTICES.md'; do
  case "$push_paths" in
    *"$required"*) pass "the Windows push filter covers $required" ;;
    *) fail "the Windows push filter must cover $required" "paths: $push_paths" ;;
  esac
done

# The fast lane is the other half of the contract: the required checks report on every pull
# request, so they must carry no path filter at all.
for fast in .github/workflows/ci.yml .github/workflows/rust.yml; do
  if python3 - "$fast" <<'PY'
import sys, yaml
d = yaml.safe_load(open(sys.argv[1]))
on = d.get(True) or d.get("on") or {}
pr = on.get("pull_request")
sys.exit(0 if isinstance(pr, dict) and "paths" not in pr or pr is None else 1)
PY
  then
    pass "$fast reports on every pull request"
  else
    fail "$fast must not be path-filtered" "a required context that never runs is never reported"
  fi
done

printf '\n%s passed, %s failed\n' "$pass_count" "$fail_count"
((fail_count == 0)) || exit 1
echo "Lane split policy tests passed"
