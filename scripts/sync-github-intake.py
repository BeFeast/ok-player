#!/usr/bin/env python3
"""Replay-safe GitHub issue/PR intake into the canonical Forgejo tracker.

GitHub is read-only. Pull requests become triage issues containing source/head
links; this script never checks out or executes their code. Existing canonical
issue state, title, labels and body are never overwritten. Run only one instance
at a time (the scheduled workflow uses a concurrency group).
"""
import argparse
import collections
import datetime
import hashlib
import json
import os
import re
import sys
import urllib.error
import urllib.parse
import urllib.request

REPO = 'BeFeast/ok-player'
# The initial full import preserved every issue/PR number through this boundary.
# Subsequent entries are mapped by immutable source ID, never by matching numbers.
HISTORICAL_LAST = 782
ECHO_MARKER = '<!-- forgejo-downstream-validation -->'
# Immutable GitHub account ID of the existing authorized downstream publisher.
DOWNSTREAM_AUTHOR_IDS = frozenset({51094})
# Immutable Forgejo writer IDs and narrow historical migration receipts.
# Account 4 is the ongoing bot; account 1 created only the bounded delta below.
IMPORTER_ID = 4
MIGRATED_ISSUE_IDS = frozenset({6387, 6388, 6389})
MIGRATED_COMMENT_IDS = (14227, 15893)
HISTORICAL_COMMENT_IDS = (4247, 5322)


def trusted_issue_record(row):
    author = row.get('user', {}).get('id')
    return (author == IMPORTER_ID
            or (author == 1 and row.get('id') in MIGRATED_ISSUE_IDS))


def trusted_comment_record(row):
    author = row.get('user', {}).get('id')
    identity = row.get('id', 0)
    return (author == IMPORTER_ID
            or (author == 1 and MIGRATED_COMMENT_IDS[0] <= identity <= MIGRATED_COMMENT_IDS[1]))


def trusted_historical_comment(row):
    # Ghost is the original full-import attribution, never a generic trust grant.
    return (row.get('user', {}).get('id') == -1
            and HISTORICAL_COMMENT_IDS[0] <= row.get('id', 0) <= HISTORICAL_COMMENT_IDS[1]
            and issue_number(row) <= HISTORICAL_LAST)



def issue_marker(source):
    return f'<!-- github-issue:{source["id"]} -->'


def comment_marker(source):
    return f'<!-- github-comment:{source["id"]} -->'


def issue_number(comment):
    url = comment.get('issue_url') or comment.get('pull_request_url') or ''
    match = re.search(r'/(?:issues|pulls)/(\d+)$', url)
    if not match:
        raise RuntimeError('Comment has no valid issue identity')
    return int(match.group(1))


def terminal_marker(body, kind):
    text = body or ''
    if kind == 'issue' and f'Imported from [https://github.com/{REPO}/' not in text:
        return None
    matches = re.findall(r'^<!-- github-' + kind + r':(\d+) -->[ \t]*$', text, re.MULTILINE)
    return int(matches[-1]) if matches else None


def map_issues(source, destination):
    by_number = {row['number']: row for row in destination}
    by_source = {}
    for row in destination:
        identity = terminal_marker(row.get('body'), 'issue') if trusted_issue_record(row) else None
        if identity is not None:
            if identity in by_source:
                raise RuntimeError('Duplicate canonical source mapping; stop for reconciliation')
            by_source[identity] = row['number']
    mapping = {}
    missing = []
    for row in source:
        if (row.get('user', {}).get('id') in DOWNSTREAM_AUTHOR_IDS
                and re.search(r'^' + re.escape(ECHO_MARKER) + r'\s*$', row.get('body') or '', re.MULTILINE)):
            continue
        if row['id'] in by_source:
            mapping[row['number']] = by_source[row['id']]
        elif row['number'] <= HISTORICAL_LAST and row['number'] in by_number:
            mapping[row['number']] = row['number']
        else:
            missing.append(row)
    return mapping, missing


def new_issue_body(source, pull=None):
    body = (source.get('body') or '')
    details = ''
    if pull:
        head = pull['head']
        sha = head['sha']
        if not re.fullmatch(r'[0-9a-f]{40,64}', sha):
            raise RuntimeError('Invalid pull request head SHA')
        details = (f'\n\n## External pull request triage\n'
                   f'Head commit: `{sha}`; head ref: `{head["ref"]}`.\n'
                   'Review the linked proposal in the canonical tracker before porting it. '
                   'No source code was fetched or executed by intake.\n'
                   f'<!-- github-pr-head:{source["id"]}:{sha} -->\n')
    return (body + details + '\n\n---\n'
            f'Imported from [{source["html_url"]}]({source["html_url"]}). '
            f'Original author: `{source["user"]["login"]}`; '
            f'created {source["created_at"]}; source last updated {source["updated_at"]}.\n'
            + issue_marker(source) + '\n')


def comment_body(source):
    # Append our source ID after quoted source text; the final marker wins.
    digest = hashlib.sha256((source.get('body') or '').encode()).hexdigest()
    return (f'Imported GitHub comment by `{source["user"]["login"]}` on '
            f'{source["created_at"]} (source last updated {source["updated_at"]}). '
            f'[Original comment]({source["html_url"]}).\n\n'
            + (source.get('body') or '')
            + f'\n\n<!-- github-revision:{digest} -->\n'
            + comment_marker(source) + '\n')


def timestamp_identity(value):
    try:
        parsed = datetime.datetime.fromisoformat(value.replace('Z', '+00:00'))
    except (ValueError, TypeError, AttributeError):
        raise RuntimeError('Invalid comment timestamp') from None
    if parsed.tzinfo is None:
        raise RuntimeError('Comment timestamp lacks timezone')
    return parsed.astimezone(datetime.timezone.utc).isoformat()


def source_token(environment):
    token = environment.get('INTAKE_GITHUB_TOKEN', '')
    if not token:
        raise RuntimeError('INTAKE_GITHUB_TOKEN is required for authenticated source scans')
    return token


def missing_comments(source, destination, mapping):
    # Historical import had no source-ID markers; exact body/time/issue matches
    # preserve those copies. Source-ID copies also recognize the migration's
    # original attribution format, before revision hashes were introduced.
    exact = collections.Counter()
    imported = collections.defaultdict(list)
    for row in destination:
        identity = terminal_marker(row.get('body'), 'comment')
        if identity is None and trusted_historical_comment(row):
            exact[(issue_number(row), timestamp_identity(row['created_at']), row.get('body') or '')] += 1
        elif identity is not None and trusted_comment_record(row):
            imported[identity].append(row)
    missing = []
    for row in source:
        target = mapping.get(issue_number(row))
        if target is None:
            continue
        body = row.get('body') or ''
        key = (target, timestamp_identity(row['created_at']), body)
        if exact[key]:
            exact[key] -= 1
            continue
        digest = hashlib.sha256(body.encode()).hexdigest()
        matches = []
        for old in imported[row['id']]:
            if issue_number(old) != target:
                raise RuntimeError('Comment source mapping points to another issue')
            text = old['body']
            revision = f'<!-- github-revision:{digest} -->\n{comment_marker(row)}'
            legacy = f'\n\n{body}\n\n{comment_marker(row)}\n'
            if revision in text or text.endswith(legacy):
                matches.append(old)
        if len(matches) > 1:
            raise RuntimeError('Duplicate imported comment revision; stop for reconciliation')
        if not matches:
            missing.append((target, row))
    return missing


class API:
    def __init__(self, base, token, writable=False):
        self.base = base.rstrip('/')
        self.token = token
        self.writable = writable

    def call(self, method, path, payload=None):
        if method != 'GET' and not self.writable:
            raise RuntimeError('Source API is strictly read-only')
        headers = {'Accept': 'application/json', 'User-Agent': 'ok-player-github-intake'}
        if self.token:
            headers['Authorization'] = 'Bearer ' + self.token
        if payload is not None:
            headers['Content-Type'] = 'application/json'
        request = urllib.request.Request(self.base + path, method=method, headers=headers,
                                         data=None if payload is None else json.dumps(payload).encode())
        try:
            with urllib.request.urlopen(request, timeout=45) as response:
                data = response.read()
                return json.loads(data) if data else None
        except urllib.error.HTTPError as error:
            raise RuntimeError(f'API {method} failed with HTTP {error.code}; rerun after inspection') from None
        except (urllib.error.URLError, TimeoutError):
            # Never blindly retry a mutation whose response might have been lost.
            raise RuntimeError('API outcome uncertain; inspect before rerunning') from None

    def pages(self, path):
        result = []
        page = 1
        while True:
            sep = '&' if '?' in path else '?'
            rows = self.call('GET', path + f'{sep}per_page=100&limit=100&page={page}')
            if not rows:
                return result
            result.extend(rows)
            page += 1


def sync(github, forgejo, apply=False):
    if apply and forgejo.call('GET', '/user').get('id') != IMPORTER_ID:
        raise RuntimeError('Forgejo write credential is not the configured importer identity')
    path = '/repos/' + REPO
    source = github.pages(path + '/issues?state=all')
    destination = forgejo.pages(path + '/issues?state=all')
    mapping, missing = map_issues(source, destination)
    report = {'apply': apply, 'new_issues': [], 'new_comments': 0, 'pr_head_revisions': 0}
    for row in sorted(missing, key=lambda item: item['number']):
        pull = github.call('GET', path + f'/pulls/{row["number"]}') if row.get('pull_request') else None
        payload = {'title': ('[GitHub PR] ' if pull else '') + row['title'],
                   'body': new_issue_body(row, pull), 'closed': row['state'] == 'closed'}
        target = None
        if apply:
            # Do not copy source assignees/ready labels into the execution queue.
            created = forgejo.call('POST', path + '/issues', payload)
            target = created['number']
            mapping[row['number']] = target
        report['new_issues'].append({'github': row['number'], 'forgejo': target})
    source_comments = github.pages(path + '/issues/comments')
    destination_comments = forgejo.pages(path + '/issues/comments')
    # Open/reopened historical PRs and post-cutover PRs receive head revisions.
    # Closed historical imported PRs stay archived until reopened.
    new_numbers = {row['number'] for row in missing}
    for row in source:
        target = mapping.get(row['number'])
        if (not row.get('pull_request') or target is None
                or (row['number'] <= HISTORICAL_LAST and row['state'] == 'closed')
                or row['number'] in new_numbers):
            continue
        pull = github.call('GET', path + f'/pulls/{row["number"]}')
        sha = pull['head']['sha']
        if not re.fullmatch(r'[0-9a-f]{40,64}', sha):
            raise RuntimeError('Invalid pull request head SHA')
        marker = f'<!-- github-pr-head:{row["id"]}:{sha} -->'
        seen = any(marker in (item.get('body') or '') for item in destination if item['number'] == target and trusted_issue_record(item))
        seen = seen or any(marker in (item.get('body') or '') for item in destination_comments
                           if issue_number(item) == target and trusted_comment_record(item))
        if not seen:
            text = (f'External [GitHub pull request]({row["html_url"]}) now has head `{sha}` '
                    f'on `{pull["head"]["ref"]}`. Source author: `{row["user"]["login"]}`. '
                    'This is a new triage revision; no code was fetched or executed.\n\n' + marker + '\n')
            if apply:
                forgejo.call('POST', path + f'/issues/{target}/comments', {'body': text})
            report['pr_head_revisions'] += 1
    comments = missing_comments(source_comments, destination_comments, mapping)
    for target, row in comments:
        if apply:
            forgejo.call('POST', path + f'/issues/{target}/comments', {'body': comment_body(row)})
    report['new_comments'] = len(comments)
    report['unmapped_source_comments'] = sum(issue_number(row) not in mapping for row in source_comments)
    return report


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--apply', action='store_true', help='Create missing Forgejo records; default is read-only')
    args = parser.parse_args()
    base = os.environ['FORGEJO_API_URL']
    parsed = urllib.parse.urlsplit(base)
    if parsed.scheme != 'https' or parsed.username or parsed.password:
        raise RuntimeError('FORGEJO_API_URL must be HTTPS without embedded credentials')
    github = API('https://api.github.com', source_token(os.environ))
    forgejo = API(base, os.environ['FORGEJO_TOKEN'], writable=args.apply)
    print(json.dumps(sync(github, forgejo, args.apply), indent=2))


if __name__ == '__main__':
    try:
        main()
    except (RuntimeError, KeyError) as error:
        print(str(error) if isinstance(error, RuntimeError) else 'Missing required configuration', file=sys.stderr)
        sys.exit(1)
