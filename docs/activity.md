# Activity

Activity is where a person investigates what agents actually did and thought: every agent session, every turn, and
every message, thinking block, tool call, and result, laid out like Slack. It is for auditing and insight, a full
investigable record rather than a dashboard, and it sits beside the main workflow, which is talking to Elysia.

This doc is the design. Coding sessions already record their full stream in `session_events`; the rest is roadmap,
in the Activity milestone.

## One log beneath the runner

- Every turn the runner runs records its full event stream in one log, whoever started it: coding sessions, Studio,
  Elysia, turn summaries, meeting segmentation, and the engine's Project Managers, workers, and triage agents.
- The log is keyed by agent session and turn, and holds the normalized events (`docs/coding.md`) with the raw line
  each came from. Secret values are scrubbed before anything is stored, as they are today.

## Sessions from outside Elysium

- A Claude Code plugin from Elysium captures a person's own sessions, opted into per person. Its hooks (session start,
  each prompt, each tool use, stop) post the transcript entries added since the last post, signed in as the person
  with a personal token. Entries are posted idempotently, so a repeated post records nothing twice.
- A person can pause capture or limit it to chosen directories. A captured session appears under its person and,
  when its working directory belongs to one of a project's repositories, under that project too.

## The page

- A sidebar groups sessions by project, like Slack channels: each project lists its sessions of every kind, with
  sessions belonging to no project under their person.
- A session opens to its whole history: turns collapse to their summaries, as GitHub Actions collapses steps, and
  expand to every message, thinking block, tool call, and result. Open sessions update live over the event stream.
- Search and filters span everything: text, agent kind, tool, person, project, and time.
- Everyone in the workspace sees every session, as they see every coding session today and everything in the
  knowledge base (`docs/knowledge.md`). A log holds whatever its agent read, so a turn that read a person's mail or
  messages shows them; capturing one's own sessions is opted into knowing that.

## Retention

Full streams are kept for a number of days that an admin sets, 365 by default; zero or less keeps them forever. Past
it, a session's stream is deleted whole, and no summary is kept in its place. Deleting a session deletes its log.
