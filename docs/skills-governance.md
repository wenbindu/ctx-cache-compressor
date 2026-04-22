# Skills Governance

## Purpose

This document defines how to manage skills used by this project so they remain discoverable, reusable, and maintainable instead of turning into an unstructured pile of prompts and references.

## Operating model

Use two layers of storage:

1. Runtime skills
   - Store reusable, auto-discoverable skills in an agent runtime directory such as `~/.codex/skills`
   - Use this location only for skills that should be available across sessions or repositories

2. Project governance
   - Store management artifacts in this repository under `docs/`
   - Use the repo as the source of truth for decisions, status, ownership, and next steps

This split keeps execution convenient while preserving project-level accountability.

## Layering model

Manage skills in three layers.

### Foundation

Cross-project rules and systems that many pages or workflows should share.

Examples:
- design language
- theming rules
- i18n standards
- evaluation baselines

### Domain

Skills tied to this product or business domain.

Examples:
- `ctx-cache-compressor` observability UI semantics
- compression-specific state and terminology
- session/turn/debug concepts

### Workflow

Task-specific execution guides that tell Codex how to perform a bounded job.

Examples:
- migrate a static playground to Next.js
- build a LiveKit-style demo page
- run a design review against this design language

## Skill lifecycle

Use these states in the catalog:

- `proposal`: useful idea, not yet created
- `draft`: created but still evolving
- `active`: approved for regular use
- `deprecated`: still present for compatibility, but should not grow
- `archived`: retained only for history or migration support

## Creation rules

Create a new skill only when at least one condition is true:

1. The workflow repeats often enough that ad hoc prompting is wasteful
2. The task needs non-obvious procedural knowledge
3. The task benefits from reusable scripts, references, or assets
4. The boundary is clearer than “miscellaneous frontend stuff”

Do not create a new skill just because a topic has multiple subtopics. Prefer references within one skill before splitting into several skills.

## Split rules

Split an existing skill only when the boundary is operationally real.

Good reasons to split:
- different owner
- different lifecycle
- different stack
- different trigger phrases
- different validation method

Bad reasons to split:
- the document became long before references were used
- two examples look visually different but follow the same design language
- the naming feels crowded

## Validation rules

Before marking a skill `active`, verify:

1. `SKILL.md` frontmatter parses cleanly
2. References are one level deep and clearly linked from `SKILL.md`
3. The skill can be used on at least one realistic task
4. The asset or example set matches the intended scope

If script-based validation fails because of environment issues, record that explicitly in the catalog instead of silently assuming the skill is fine.

## Naming rules

- Use lowercase hyphen-case
- Prefer short, action- or domain-led names
- Avoid stacking several unrelated concerns into one name
- Keep names stable once a skill becomes `active`

## Ownership

Each skill should have one owner field in the catalog, even if the owner is a team or a repository.

Suggested owner values:
- `global`
- `ctx-cache-compressor`
- `<team-name>`
- `<person-name>`

## Review cadence

Review the catalog when:

- a new skill is proposed
- a skill changes layer
- a skill becomes stale or duplicated
- a large UI or architecture direction changes

## Current recommendation for this project

Start with:

1. one foundation skill for shared frontend design language
2. later add domain skills only when `ctx-cache-compressor` product semantics become stable
3. add workflow skills only when concrete repeated jobs emerge

This prevents premature fragmentation while still giving the project a clean path to scale its skill system.
