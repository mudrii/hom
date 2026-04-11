---
name: hom-workspace-standards
description: Use before making meaningful code changes in HOM to load repository architecture, workflow rules, crate boundaries, feature flags, and verification requirements
---

<objective>
Load the repository-specific rules that apply across the HOM workspace.

This is the Codex-compatible form of the important stable guidance from `CLAUDE.md`.
</objective>

<when_to_use>
Use this skill for:
- any feature work
- any bugfix
- any refactor
- any architecture review
- any change that crosses crate boundaries
</when_to_use>

<required_reading>
Read:
- `skills/hom-workspace-standards/references/workspace-architecture.md`

Then read the relevant domain skill for the part of the codebase you are touching.
</required_reading>

<process>
Follow this baseline workflow:

1. Inspect the workspace root, relevant crate layout, feature flags, and tests before editing.
2. Match existing patterns and prefer the smallest coherent change.
3. Keep behavior changes, tests, and docs in the same change.
4. Pass dependencies explicitly; do not hardcode ports, paths, URLs, credentials, or feature switches.
5. Verify locally before handing off.
</process>

<success_criteria>
This skill is being followed correctly when:
- crate boundaries remain intact
- project-specific architecture constraints are preserved
- feature flag boundaries remain clean
- tests and verification match the affected scope
- the change does not quietly bypass existing configuration or runtime wiring
</success_criteria>
