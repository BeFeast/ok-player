#!/usr/bin/env python3
"""Behavioral tests for immutable source mapping and replay-safe intake."""
import copy
import importlib.util
from pathlib import Path
import unittest

SPEC = importlib.util.spec_from_file_location('intake', Path(__file__).with_name('sync-github-intake.py'))
intake = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(intake)


def issue(number=800, identity=9000, body='External report'):
    return {'number': number, 'id': identity, 'body': body, 'title': 'Source title',
            'state': 'open', 'html_url': f'https://github.com/BeFeast/ok-player/issues/{number}',
            'user': {'login': 'contributor'}, 'created_at': '2026-09-07T00:00:00Z',
            'updated_at': '2026-09-07T00:00:00Z'}


def comment(body='Please fix', identity=321, number=800):
    return {'id': identity, 'body': body, 'issue_url': f'https://api.github.com/repos/BeFeast/ok-player/issues/{number}',
            'html_url': f'https://github.com/BeFeast/ok-player/issues/{number}#issuecomment-{identity}',
            'user': {'login': 'contributor'}, 'created_at': '2026-09-07T00:01:00Z',
            'updated_at': '2026-09-07T00:01:00Z'}


class FakeAPI:
    def __init__(self, issues, comments=()):
        self.issues = copy.deepcopy(issues)
        self.comments = copy.deepcopy(list(comments))
        self.calls = []
        self.head_sha = 'a' * 40

    def pages(self, path):
        return copy.deepcopy(self.comments if path.endswith('/comments') else self.issues)

    def call(self, method, path, payload=None):
        self.calls.append((method, path))
        if method == 'GET':
            return {'head': {'sha': self.head_sha, 'ref': 'untrusted-branch'}}
        if path.endswith('/comments'):
            row = {'id': 10000 + len(self.comments), 'body': payload['body'],
                   'created_at': '2026-09-08T00:00:00Z', 'issue_url': 'https://forge.invalid' + path[:-9]}
            self.comments.append(row)
            return row
        row = dict(payload, number=max((row['number'] for row in self.issues), default=0) + 1)
        self.issues.append(row)
        return row


class IntakeTests(unittest.TestCase):
    def test_collision_maps_by_source_id_and_replay_preserves_canonical_edits(self):
        source = FakeAPI([issue()], [comment()])
        canonical = issue(body='Canonical unrelated issue', identity=88)
        target = FakeAPI([canonical])
        first = intake.sync(source, target, apply=True)
        self.assertEqual(first['new_issues'], [{'github': 800, 'forgejo': 801}])
        self.assertEqual(first['new_comments'], 1)
        target.issues[1]['state'] = 'closed'
        target.issues[1]['title'] = 'Canonical rewritten title'
        target.issues[1]['body'] += '\nCanonical investigation notes appended after intake.\n'
        second = intake.sync(source, target, apply=True)
        self.assertEqual(second['new_issues'], [])
        self.assertEqual(second['new_comments'], 0)
        self.assertEqual(target.issues[0], canonical)
        self.assertEqual(target.issues[1]['title'], 'Canonical rewritten title')
        self.assertEqual(target.issues[1]['state'], 'closed')
        self.assertEqual(len(target.comments), 1)
        self.assertEqual(source.calls, [])

    def test_updated_source_comment_adds_one_revision_without_overwriting(self):
        source = FakeAPI([issue()], [comment()])
        target = FakeAPI([])
        intake.sync(source, target, True)
        original = copy.deepcopy(target.comments[0])
        source.comments[0]['body'] = 'Updated reproduction'
        self.assertEqual(intake.sync(source, target, True)['new_comments'], 1)
        self.assertEqual(intake.sync(source, target, True)['new_comments'], 0)
        self.assertEqual(target.comments[0], original)
        self.assertEqual(len(target.comments), 2)

    def test_legacy_comment_and_attributed_migration_are_not_duplicated(self):
        original = comment(number=42)
        legacy = copy.deepcopy(original)
        attributed = comment(identity=322, number=42)
        imported = dict(attributed, created_at='2026-09-08T00:00:00Z',
                        body='Imported GitHub comment\n\nPlease fix\n\n<!-- github-comment:322 -->\n')
        self.assertEqual(intake.missing_comments([original, attributed], [legacy, imported], {42: 42}), [])

    def test_new_pr_is_triage_issue_only_and_echo_is_skipped(self):
        proposal = dict(issue(), pull_request={'url': 'unused'})
        echo = issue(801, 9001, 'Validation companion for the canonical Forgejo pull request.\n\n'
                     + intake.ECHO_MARKER + '\n\n<!-- CURSOR_SUMMARY -->\n---\n'
                     + '> Reviewed by Cursor Bugbot.\n<!-- /CURSOR_SUMMARY -->')
        source = FakeAPI([proposal, echo])
        target = FakeAPI([])
        intake.sync(source, target, True)
        self.assertEqual(len(target.issues), 1)
        self.assertEqual(target.issues[0]['title'], '[GitHub PR] Source title')
        self.assertIn('a' * 40, target.issues[0]['body'])
        self.assertNotIn('labels', target.issues[0])
        self.assertEqual(source.calls, [('GET', '/repos/BeFeast/ok-player/pulls/800')])

    def test_external_pr_head_change_adds_exactly_one_triage_revision(self):
        source = FakeAPI([dict(issue(), pull_request={'url': 'unused'})])
        target = FakeAPI([])
        intake.sync(source, target, True)
        original = copy.deepcopy(target.issues)
        self.assertEqual(intake.sync(source, target, True)['pr_head_revisions'], 0)
        source.head_sha = 'b' * 40
        self.assertEqual(intake.sync(source, target, True)['pr_head_revisions'], 1)
        self.assertEqual(intake.sync(source, target, True)['pr_head_revisions'], 0)
        self.assertEqual(target.issues, original)
        self.assertEqual(len(target.comments), 1)
        self.assertIn('b' * 40, target.comments[0]['body'])
        self.assertTrue(all(method == 'GET' for method, _ in source.calls))

    def test_duplicate_issue_mapping_stops_instead_of_overwriting(self):
        row = issue()
        body = intake.new_issue_body(row)
        with self.assertRaises(RuntimeError):
            intake.map_issues([row], [dict(row, number=800, body=body), dict(row, number=801, body=body)])

    def test_embedded_source_marker_does_not_claim_mapping(self):
        row = issue()
        mapping, missing = intake.map_issues([row], [issue(body=intake.issue_marker(row) + '\nquoted source text')])
        self.assertEqual(mapping, {})
        self.assertEqual(missing, [row])

    def test_source_client_refuses_mutation(self):
        with self.assertRaises(RuntimeError):
            intake.API('https://api.github.com', '').call('POST', '/anything', {})


if __name__ == '__main__':
    unittest.main()
