# Documentation map

[Project overview](../README.md) · [Agent instructions](../AGENTS.md)

This repository separates current behavior, future design, and history.
Do not copy outdated layouts from historical notes into new tests.

| Document | Responsibility | Status |
| --- | --- | --- |
| [Usage](usage.md) | Commands, configuration, currently working controls | **Current** (verify source) |
| [Architecture](architecture.md) | Modules, data flow, persistent source-position invariants | **Current** (verify source) |
| [Development](development.md) | Build, verification, documentation rules | Working procedure |
| [Windows build](Windows构建环境.md) | MSVC/GNU setup and troubleshooting | Platform reference |
| [Agent Terminal design](design/agent-terminal-reading.md) | Low-exposure/readability UX goals, anti-patterns, acceptance criteria | **Target (not implemented)** |
| [Reading-experience notes](notes/阅读体验优化讨论.md) | Past alternatives, experiments, work reports | **Historical only** |
| [AGENTS.md](../AGENTS.md) | Agent-specific task and safety constraints | Working contract |

## Where to begin

- **Agent UX implementation:** design contract → architecture → current source
  and tests; replace outdated acceptance tests, not merely UI labels.
- **Reader controls and CLI:** usage → relevant source → tests.
- **Data and parsing:** architecture → position/migration tests.
- **Platform setup:** development → Windows build reference.
- **Design history:** notes for context only, not acceptance requirements.

## Editing discipline

- **Implementation truth** comes from code and executable tests.
- **UX intent** comes from the design contract; old code is not necessarily
  the target for the next version.
- **Historical context** is deliberately retained, but is not normative.
- Update one authoritative location for each requirement and link from
  other documents. Do not create a new directory for each discussion.
