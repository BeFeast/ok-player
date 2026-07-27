## What this changes

<!-- One paragraph, in terms of user-visible or operator-visible behaviour. -->

## How it was verified

<!--
Name the evidence, not the intention. For a bug fix, the required evidence is:
the regression test fails with the production change reverted, and passes with
it applied. Paste the failing assertion.

A test that only asserts that a source file contains a string is not evidence;
CI rejects new ones (scripts/check-source-grep-tests.py).
-->

## What this deliberately does not do

<!-- Scope you consciously left out, and why. "Nothing" is a valid answer. -->

## Notes for review

<!--
Optional. If this change needs an operator to accept it before it may merge,
move the block below out of this comment and leave the boxes unchecked - CI
blocks the merge until every box is ticked.

## Operator acceptance

- [ ] Packaged build installed and launched on the target desktop
- [ ] Behaviour verified by hand, evidence linked

That block may contain checkboxes and nothing else: a plain bullet or a sentence
states a condition that nothing can ever record as performed, and CI rejects it
even when other boxes in the block are ticked. Put context above the block.

Never tick a box you did not perform. Never delete an acceptance block to get a
green check. The same applies to the maestro WIP marker comment: it blocks the
merge for as long as it is in the body, and removing it is a statement that the
work behind it is done.
-->
