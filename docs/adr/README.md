# Architecture Decision Records

ADRs preserve the reasoning behind decisions that are expensive to reverse or easy to misunderstand later.

Create an ADR when a decision changes a canonical representation, public semantic contract, cross-crate boundary, extension model, or another choice that future contributors are likely to revisit.

Do not create ADRs for routine refactors, naming changes, small implementation details, or decisions that can be reversed cheaply.

## Naming

Use monotonically increasing numbers:

```text
0001-short-title.md
0002-short-title.md
```

## Template

```markdown
# ADR NNNN: Title

Status: Proposed | Accepted | Superseded
Date: YYYY-MM-DD

## Context

What problem or constraint requires a decision?

## Decision

What are we choosing?

## Alternatives considered

What credible alternatives were rejected and why?

## Consequences

What becomes easier, harder, or constrained by this decision?

## Revisit when

What evidence or future condition should cause this decision to be reconsidered?
```

When an ADR is superseded, keep the old record and link to the replacement rather than rewriting history.
