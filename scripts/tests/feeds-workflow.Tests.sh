#!/usr/bin/env bash
# The APT archive is built on the internal builder (the only machine that can
# reach the signing key) and travels to the hosted deploy job as an artifact.
# That handoff has ordering constraints which are invisible until a Pages
# deployment fails, so they are asserted here: the archive must be downloaded
# before anything reads or publishes it, and the key must never be handed to a
# hosted runner.
#
# Since issue #689 the archive carries two suites (stable and candidate). Both
# are produced and signed by ONE invocation of build-apt-repo.sh in the signing
# job — a second archive-building step, especially one on a hosted runner, would
# mean a second signing path. And the candidate suite is only as fresh as the
# last Pages deploy, so a rolling publication that does not trigger one leaves
# testers' `apt upgrade` on the previous build; that trigger is asserted too.
#
# "A step mentions the script" is not the same as "a step runs the script":
# `echo verify-apt-repo.sh disabled` mentions it. So the assertions below parse
# each run: block into commands and look at what is actually being executed.
set -uo pipefail
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
WF="$ROOT/.github/workflows/publish-update-feeds.yml"
CANDIDATE_WF="$ROOT/.github/workflows/release-linux-candidate.yml"

python3 - "$WF" "$CANDIDATE_WF" <<'PY'
import os, re, shlex, sys, yaml

wf = yaml.safe_load(open(sys.argv[1]))
candidate_wf = yaml.safe_load(open(sys.argv[2]))
jobs = wf["jobs"]
fail = []

def step_names(job):
    return [s.get("name") or s.get("uses") or "" for s in job["steps"]]

def index_of(names, needle):
    for i, n in enumerate(names):
        if needle in n:
            return i
    return -1

def body(job):
    return yaml.safe_dump(job)

def commands(run):
    """Every command a run: block executes, as token lists."""
    for raw in (run or "").splitlines():
        line = raw.strip()
        if not line or line.startswith("#"):
            continue
        # Only unconditionally executed commands count. A step written as
        # `false && ./scripts/verify-apt-repo.sh` mentions the verifier and
        # parses as a command, yet can never run it, so anything guarded by
        # a preceding && or || is deliberately not credited.
        head = re.split(r"&&|\|\|", line)[0]
        for part in re.split(r";|\|", head):
            try:
                tokens = shlex.split(part)
            except ValueError:
                continue
            # Skip leading VAR=value assignments and common prefixes, so
            # `sudo ./scripts/x.sh` and `FOO=1 ./scripts/x.sh` still count.
            i = 0
            while i < len(tokens) and re.match(r"^[A-Za-z_][A-Za-z0-9_]*=", tokens[i]):
                i += 1
            while i < len(tokens) and tokens[i] in ("sudo", "env", "bash", "sh", "exec"):
                i += 1
            if i < len(tokens):
                yield tokens[i:]

def invokes(job, script):
    """True when some step of the job actually executes <script>."""
    for step in job.get("steps", []):
        for tokens in commands(step.get("run")):
            if os.path.basename(tokens[0]) == script:
                return True
    return False

# 1. The signing job runs where the key host resolves, and nowhere else.
apt = jobs.get("apt-repository")
if apt is None:
    fail.append("the apt-repository job is gone; signing would move back to a hosted runner")
else:
    runs_on = apt.get("runs-on")
    if not (isinstance(runs_on, list) and "self-hosted" in runs_on):
        fail.append(f"apt-repository must run on a self-hosted runner, got {runs_on!r}")

pub = jobs.get("publish-feeds")
if pub is None:
    fail.append("publish-feeds job is gone")
else:
    # 2. The deploy job waits for the archive.
    needs = pub.get("needs")
    needs = [needs] if isinstance(needs, str) else (needs or [])
    if "apt-repository" not in needs:
        fail.append(f"publish-feeds must depend on apt-repository, needs={needs!r}")

    names = step_names(pub)
    dl = index_of(names, "download-artifact")
    verify = index_of(names, "Verify the APT repository")
    upload = index_of(names, "upload-pages-artifact")

    # 3. Nothing may read or publish the archive before it exists locally.
    if dl < 0:
        fail.append("publish-feeds never downloads the apt-repository artifact")
    else:
        if verify >= 0 and dl > verify:
            fail.append("the archive is verified before it is downloaded: the verifier reads _site/apt, which only the artifact provides")
        if upload >= 0 and dl > upload:
            fail.append("the site is uploaded before the archive is merged into it")

    # 4. The signing secrets must never reach a hosted runner.
    if "INFISICAL_CLIENT_SECRET" in body(pub):
        fail.append("publish-feeds references the Infisical credentials; the key must stay on the internal builder")

    # 5. apt itself is the only authority on whether the archive works, and it is
    #    asked once per deploy, on the hosted runner that holds the merged site.
    if not invokes(pub, "verify-apt-repo.sh"):
        fail.append("no step in publish-feeds executes verify-apt-repo.sh; an unverified archive would reach Pages")

# 6. One generator, two suites. Every suite in the archive must come out of the
#    same run of build-apt-repo.sh, so there is exactly one place where the
#    signing key is used and exactly one archive layout to reason about.
builders = sorted(name for name, job in jobs.items() if invokes(job, "build-apt-repo.sh"))
if builders != ["apt-repository"]:
    fail.append(
        "build-apt-repo.sh must be executed exactly once, by apt-repository; executed by "
        f"{builders!r}. Two invocations mean two signing paths and two archives."
    )

# 7. A rolling candidate publication has to reach the archive. publish-update-feeds is the only
#    writer of the Pages site, and replacing assets on the mutable linux-candidate release fires
#    no event it listens for, so without this call the APT candidate suite would only refresh when
#    something unrelated deployed. It must also be conditional: the candidate workflow runs every
#    15 minutes and most runs publish nothing.
refreshers = {
    name: job
    for name, job in candidate_wf["jobs"].items()
    if str(job.get("uses", "")).endswith("publish-update-feeds.yml")
}
if not refreshers:
    fail.append(
        "release-linux-candidate.yml never calls publish-update-feeds.yml; a new rolling candidate "
        "would not reach the APT candidate suite until an unrelated deploy happened to run"
    )
# 8. The refresh must not hold the native builder's concurrency group. A workflow-level group
#    covers every job in the run, so a Pages deploy that cannot start — signing runner offline,
#    secret store down — would stop candidate builds entirely. The QA lane must not be blocked by
#    the publication of its own archive, so the group belongs to the building job.
if candidate_wf.get("concurrency"):
    fail.append(
        "release-linux-candidate.yml declares a workflow-level concurrency group, which the feed "
        "refresh would hold for the length of a Pages deploy; scope it to the building job"
    )
if not candidate_wf["jobs"].get("build-and-publish", {}).get("concurrency"):
    fail.append(
        "the native build/publish job has no concurrency group; two builders would run at once"
    )

for name, job in refreshers.items():
    if job.get("secrets") != "inherit":
        fail.append(f"{name} must pass secrets: inherit, or the signing key cannot be fetched")
    condition = str(job.get("if", ""))
    produced = [n for n in ([job.get("needs")] if isinstance(job.get("needs"), str) else (job.get("needs") or []))]
    if not produced:
        fail.append(f"{name} does not depend on the publishing job, so it cannot know whether anything was published")
    if not condition or not any(f"needs.{n}.outputs" in condition for n in produced):
        fail.append(
            f"{name} must be gated on an output of {produced!r}; an unconditional refresh would "
            "redeploy Pages on every no-op scheduled run"
        )

# 9. The refresh must be *unskippable* once a promotion happened. Check 8 only asks that the
#    condition mention a needs output, and `needs: build-and-publish` paired with
#    `if: needs.build-and-publish.outputs.publish_result == 'published'` satisfies that while
#    being exactly the delivery bug: `needs` carries an implicit success() gate, so an advisory
#    gate failure — the builder stages the package, moves the marker, then exits nonzero to
#    withhold the *verdict* rather than the package — skipped the refresh and stranded a
#    published candidate outside the APT archive. Testers' `apt upgrade` then found nothing
#    while the rolling pointer already named the new build.
#
#    Asserting the fixed condition as a literal string would be satisfied by a comment that
#    happens to contain it, and broken by an equivalent rewrite. So the condition is parsed and
#    evaluated instead, against the states this lane actually reaches. That job outputs survive
#    a failed job — the premise the fix rests on — was confirmed empirically on this repo before
#    this check was written.
STATUS_FUNCS = {"success", "failure", "cancelled", "canceled", "always"}


class ExprError(Exception):
    pass


def tokenize(src):
    tokens, i = [], 0
    while i < len(src):
        c = src[i]
        if c.isspace():
            i += 1
            continue
        if src[i:i + 2] in ("&&", "||", "==", "!="):
            tokens.append(src[i:i + 2])
            i += 2
            continue
        if c in "()!":
            tokens.append(c)
            i += 1
            continue
        if c == "'":
            j, buf = i + 1, []
            while j < len(src):
                if src[j] == "'":
                    if src[j:j + 2] == "''":
                        buf.append("'")
                        j += 2
                        continue
                    break
                buf.append(src[j])
                j += 1
            else:
                raise ExprError("unterminated string literal")
            tokens.append(("str", "".join(buf)))
            i = j + 1
            continue
        m = re.match(r"[A-Za-z_][A-Za-z0-9_.\-]*", src[i:])
        if not m:
            raise ExprError(f"unexpected character {c!r}")
        tokens.append(("id", m.group(0)))
        i += len(m.group(0))
    return tokens


class Parser:
    """Just enough of the GitHub expression grammar for a job-level if:."""

    def __init__(self, tokens):
        self.t, self.i = tokens, 0

    def peek(self):
        return self.t[self.i] if self.i < len(self.t) else None

    def eat(self, expected=None):
        cur = self.peek()
        if expected is not None and cur != expected:
            raise ExprError(f"expected {expected!r}, got {cur!r}")
        self.i += 1
        return cur

    def parse(self):
        node = self.or_expr()
        if self.i != len(self.t):
            raise ExprError(f"trailing tokens at {self.peek()!r}")
        return node

    def or_expr(self):
        node = self.and_expr()
        while self.peek() == "||":
            self.eat()
            node = ("or", node, self.and_expr())
        return node

    def and_expr(self):
        node = self.cmp_expr()
        while self.peek() == "&&":
            self.eat()
            node = ("and", node, self.cmp_expr())
        return node

    def cmp_expr(self):
        node = self.unary()
        while self.peek() in ("==", "!="):
            op = self.eat()
            node = (op, node, self.unary())
        return node

    def unary(self):
        if self.peek() == "!":
            self.eat()
            return ("not", self.unary())
        return self.primary()

    def primary(self):
        cur = self.peek()
        if cur == "(":
            self.eat()
            node = self.or_expr()
            self.eat(")")
            return node
        if isinstance(cur, tuple) and cur[0] == "str":
            self.eat()
            return ("lit", cur[1])
        if isinstance(cur, tuple) and cur[0] == "id":
            self.eat()
            name = cur[1]
            if self.peek() == "(":
                self.eat()
                self.eat(")")
                return ("call", name)
            if name in ("true", "false"):
                return ("lit", name == "true")
            return ("ref", name)
        raise ExprError(f"unexpected token {cur!r}")


def truthy(value):
    return value if isinstance(value, bool) else value not in ("", None)


def evaluate(node, ctx):
    kind = node[0]
    if kind == "lit":
        return node[1]
    if kind == "ref":
        parts = node[1].split(".")
        # Only the needs context is modelled. Anything else (github.*, inputs.*) would make the
        # outcome depend on state this check cannot see, so refuse rather than guess.
        if parts[0] != "needs" or len(parts) < 3:
            raise ExprError(f"unsupported reference {node[1]!r}")
        job = parts[1]
        if parts[2] == "result":
            return ctx["results"].get(job, "")
        if parts[2] == "outputs" and len(parts) == 4:
            return ctx["outputs"].get(job, {}).get(parts[3], "")
        raise ExprError(f"unsupported reference {node[1]!r}")
    if kind == "call":
        name = node[1]
        if name == "always":
            return True
        if name in ("cancelled", "canceled"):
            return ctx["cancelled"]
        if name == "success":
            return not ctx["cancelled"] and all(r == "success" for r in ctx["results"].values())
        if name == "failure":
            return not ctx["cancelled"] and any(r == "failure" for r in ctx["results"].values())
        raise ExprError(f"unsupported function {name}()")
    if kind == "not":
        return not truthy(evaluate(node[1], ctx))
    if kind == "and":
        left = evaluate(node[1], ctx)
        return evaluate(node[2], ctx) if truthy(left) else left
    if kind == "or":
        left = evaluate(node[1], ctx)
        return left if truthy(left) else evaluate(node[2], ctx)
    if kind in ("==", "!="):
        def norm(v):
            return "" if v is None else (str(v).lower() if isinstance(v, bool) else v)
        equal = norm(evaluate(node[1], ctx)) == norm(evaluate(node[2], ctx))
        return equal if kind == "==" else not equal
    raise ExprError(f"unsupported node {kind!r}")


def mentions_status(node):
    if node[0] == "call" and node[1] in STATUS_FUNCS:
        return True
    return any(mentions_status(c) for c in node[1:] if isinstance(c, tuple))


def job_would_run(tree, ctx):
    # A job whose if: names no status function carries an implicit success() over its needs.
    # That implicit gate is the whole defect, so it has to be modelled rather than ignored.
    if not mentions_status(tree):
        gate = evaluate(("call", "success"), ctx)
        if not truthy(gate):
            return False
    return truthy(evaluate(tree, ctx))


def declared_outputs(job_name):
    return (candidate_wf["jobs"].get(job_name) or {}).get("outputs") or {}


for name, job in refreshers.items():
    needs_list = job.get("needs")
    needs_list = [needs_list] if isinstance(needs_list, str) else (needs_list or [])
    producers = [n for n in needs_list if "publish_result" in declared_outputs(n)]
    if not producers:
        fail.append(
            f"{name} depends on {needs_list!r}, none of which declares a publish_result output, "
            "so the refresh has no way to tell a promotion from a no-op scheduled run"
        )
        continue
    producer = producers[0]

    raw = job.get("if")
    expr_src = "true" if raw is None else str(raw).strip()
    if expr_src.startswith("${{") and expr_src.endswith("}}"):
        expr_src = expr_src[3:-2].strip()

    # published/red is the state the gate split deliberately produces and the one that stranded
    # packages outside the archive; skipped/green is the 15-minute no-op that must not redeploy.
    scenarios = [
        ("a candidate promoted by a build an advisory gate then reddened",
         "failure", "published", False, True),
        ("a candidate promoted by a fully green build",
         "success", "published", False, True),
        ("a scheduled run that promoted nothing",
         "success", "skipped", False, False),
        ("a build that failed before it staged anything",
         "failure", "", False, False),
        ("a superseded run that never reached the publisher",
         "success", "", False, False),
    ]

    try:
        tree = Parser(tokenize(expr_src)).parse()
    except ExprError as exc:
        fail.append(
            f"{name} has an if: this check cannot evaluate ({exc}); condition={expr_src!r}. "
            "Delivery must be provable, so an unreadable gate is treated as a failure"
        )
        continue

    for label, result, publish_result, cancelled, expected in scenarios:
        ctx = {
            "results": {n: ("success" if n != producer else result) for n in needs_list},
            "outputs": {producer: {"publish_result": publish_result}},
            "cancelled": cancelled,
        }
        try:
            actual = job_would_run(tree, ctx)
        except ExprError as exc:
            fail.append(f"{name}: could not evaluate if: for {label} ({exc})")
            break
        if actual == expected:
            continue
        if expected:
            fail.append(
                f"{name} is skipped for {label} (publish_result={publish_result!r}, "
                f"{producer}={result}). The package is already on the rolling release, so the "
                "archive would never carry it and `apt upgrade` would find nothing. "
                f"condition={expr_src!r}"
            )
        else:
            fail.append(
                f"{name} runs for {label} (publish_result={publish_result!r}, "
                f"{producer}={result}). Nothing was promoted, so this only spends a Pages "
                f"deploy on a no-op. condition={expr_src!r}"
            )

if fail:
    for f in fail:
        print("FAIL:", f, file=sys.stderr)
    raise SystemExit(1)

print("ok: one generator builds both suites on the internal builder, the archive is downloaded "
      "and verified before it is deployed, and a rolling candidate publication refreshes it - "
      "including one an advisory gate reddened after the package was already published")
PY
