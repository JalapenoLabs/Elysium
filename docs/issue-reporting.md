# Reporting problems

When something in Elysium fails and the person believes it is not theirs to fix, they press Report beside it, and
Elysium files the problem on its own repository, `JalapenoLabs/Elysium`, as a GitHub issue under their name. Ten people
hitting the same problem make one issue, opened by the first, and nine comments with each person's details.

The code is `api/src/issue_reports/`, the routes `/api/v1/issue-reports`, and the dialog
`frontend/src/components/ReportIssueModal.tsx`.

## Filed as the person, with their own token

A report is posted with one of the reporter's own GitHub tokens, the ones they added (`github_credentials.created_by`),
so GitHub shows them as its author and they can follow it. The dialog lists those tokens with the workspace default
preselected, falling back to the first that can report. Nobody else's token is offered, and a request naming one
answers `404`.

Whether a token can open issues on a public repository is told before anything is sent:

| Readiness       | Token                                               | Sent? |
|-----------------|-----------------------------------------------------|-------|
| `ready`         | Classic, with `public_repo` or `repo`               | Yes   |
| `fine_grained`  | Fine-grained                                         | Tried |
| `missing_scope` | Classic, without either scope                       | No    |
| `expired`       | Past its expiry                                     | No    |

A fine-grained token reaches only the repositories of its one resource owner, and GitHub does not report its
permissions, so it can only be tried: it works for a token made for JalapenoLabs and is refused for anyone else's.
GitHub refusing a write (`403`, or `404` for a repository the token cannot see) answers `400` with the code
`github_token_cannot_report` and says that a classic token with `public_repo` works. A person with no token of their
own is offered Add a GitHub token instead of Post.

## What a report is about

The dialog is opened with a subject (`api/src/issue_reports/subject.rs`). Where Elysium holds the thing that failed, the
subject names it and the server reads the error and its context from its own records, so a person reports only what
they may see:

| Subject          | Names                       | Reads                                                         |
|------------------|-----------------------------|---------------------------------------------------------------|
| `connectorSync`  | A connector, and a run      | The run's error, or the connector's last error (the link watcher's for Jira and GitHub); how many of its last 10 syncs failed; up to 3 other recent failures; its checks that did not pass |
| `connectorCheck` | A connector and a check     | That check's last stored result, and the connector's other checks |
| `checkRun`       | A check run and a check     | That check in a run the person started, kept for half an hour; how a connector not saved yet is reported |
| `error`          | An area and a message       | Nothing more: any other surface reports what it showed, such as an action item's pending provider write (`action_items.link_write.jira.close`) |

A check is reported to the same category whether it failed on a saved connector or one being set up, so both join one
issue. A subject with nothing to report, such as a check that passed, answers `400`; an expired run or another
person's connector, `404`.

## What is posted

`issue_reports::prepare` renders the report, a pure function of the problem, the build, the browser, the person's
note, and the redaction values, with no clock in it. The preview the dialog shows is that rendering, and filing renders
it again from the same inputs, so what is posted is exactly what was shown. The body is the same whether it opens an
issue or joins one as a comment:

- **What happened.** One sentence in Elysium's words, and the error.
- **Where.** Facts Elysium wrote itself: the connector's kind, the check and whether it is required, its reason, its
  scope, sign-in state, and whether it is paused, or the area.
- **Related.** The other checks and recent failures, one line each.
- **Environment.** The release, or the crate version and profile of a `main` build; the commit and branch it was built
  from and when, as `/api/version` reports them; and the browser as its family, major version, and operating system
  (`Firefox 145 on Linux`), never the whole `User-Agent`.
- **From the person.** Their note, if they wrote one.
- The fingerprint, on its own line: ``Fingerprint: `2cd8ee68cdf0b0ad` ``.

Every piece of text that came from outside Elysium's code (errors, related lines, the note) is redacted, then cut to
length (4,000 characters of error, 300 per related line, 2,000 of note), then set in a fenced code block. Cutting after
redacting keeps a half token from surviving, and the fence shows its contents as written: no HTML runs, no `@` notifies
anyone, no `#12` links an issue. The title is the summary and the first line of the redacted error.

### Redaction

`api/src/issue_reports/redact.rs` is deterministic: the same text always redacts to the same result. Placeholders are
words in square brackets, such as `[email]`, which no rule matches again. It errs toward hiding.

1. **URLs.** The path, query, fragment, and credentials always go (`/[path]`). The host stays only on a fixed list of
   public services (`api.github.com`, `login.microsoftonline.com`, Elysium's own compose services, ...), with its port.
   A tenant host keeps only the service it is on (`[site].atlassian.net`, also `youtrack.cloud`, `slack.com`,
   `sharepoint.com`, `amazonaws.com`, `b-cdn.net`, `github.io`); any other host is `[host]`, and an IP is `[ip]`.
2. **Teams conversation ids** (`19:...@thread.v2`), before addresses, which they resemble.
3. **Email addresses.**
4. **Secrets.** `Bearer`, `Basic`, and `token` credentials; JWTs; tokens with a known prefix (GitHub's, Slack's,
   Anthropic's, OpenAI's, AWS access key ids, Google API keys, Atlassian API tokens, GitLab's); and values after a
   secret's name (`password=`, `"access_token": `, `cookie:`), where after a colon only a value that looks issued counts,
   so `token: expired` stays.
5. **Known values.** Everything the workspace holds that names a person or an account they sign in as
   (`issue_report::personal_values`): people's names and each word of them, their addresses with the local part and the
   domain's owner unless it is a public mail provider, GitHub logins, Atlassian sites, account emails, ids, and display
   names, token accounts' identities and sites, and mailboxes. Matched case-insensitively on word boundaries, longest
   first. The seeded machines (`Elysium`, `Elysia`, `Coding agent`) and the labels people give their own accounts are
   left out, since hiding them would hide the error. A name only a pattern could find, of someone the workspace does not
   know, is not redacted; the person sees the preview before anything is posted.
6. **Ids and addresses.** UUIDs, IPv4 and IPv6 (an IPv6 only when it parses as one, so `crate::github` and `10:30:00`
   stay), home directories (`/home/[user]`), Slack ids that hold a digit, hex ids of 12 or more characters that mix
   digits and letters, strings of 24 or more that mix digits with both cases, runs of seven or more digits, and `@`
   mentions.

## One issue per problem

A problem's fingerprint (`api/src/issue_reports/fingerprint.rs`) is the first 16 hex characters of a SHA-256 over a
version tag, the category (such as `connector:github:check:sign_in`), and the redacted error normalized: lower-cased,
whitespace collapsed, every run of digits as `#`, the first 1,000 characters. Because names, addresses, and sites are
redacted first, two people's errors for the same bug fingerprint the same. The version and the browser are left out,
so one issue holds a problem across releases. Matching is exact; nothing is judged similar.

Filing (`IssueReports::file`) finds the problem's open issue:

1. The issue this deployment last filed the fingerprint to (`issue_reports`), asked of GitHub directly. GitHub's search
   index lags a new issue by up to a minute or so, and this finds it anyway.
2. Otherwise GitHub's issue search, `repo:JalapenoLabs/Elysium is:issue is:open in:body <fingerprint>`. A result counts
   only when its body holds the exact fingerprint line, so an issue merely mentioning the fingerprint never matches; of
   several, the oldest wins.

An open issue gets the report as a comment; no open issue means a new one. A closed issue means the problem was fixed,
so its return opens a new issue. Two reports from two deployments within the search index's lag can each open one;
within one deployment, filing holds a lock, since the API runs as one replica.

## Limits

- **Ten a day.** Each person may file `REPORT_LIMIT` (10) reports in a rolling 24 hours (`REPORT_WINDOW`), counted
  from `issue_reports`. Past it, filing answers `429` with the code `report_limit` and when the next can be filed,
  before GitHub is asked. The preview says how many are left.
- **Once per open issue.** A person who already reported a problem to its open issue is answered `409`
  `already_reported`, and the dialog links to their earlier report. Once that issue is closed they may report its return.

Both routes need a session, and `POST` needs Elysium's own origin, like every unsafe route (`docs/security.md`).

## In the web app

`ReportIssueGate` (`frontend/src/gates/`) holds one report dialog for the whole app, inside the router so its links
navigate in place. Any error surface opens it with `useReportIssue()(subject)`, or drops in `ReportIssueButton` with
the subject, icon-only where a row has no room. Today it sits beside a connector's last error, each failed sync in the
hub's activity feed, every failed check (a saved connector's, a run on its page, and a new connector's test run), and
an action item's pending provider write that keeps failing.

The dialog shows the person's earlier report of the problem, the token to file as and the account it files as (with
why a token cannot report), a note, and the preview. The preview is rendered again once typing in the note rests
(`ISSUE_REPORT_PREVIEW_DELAY_MS`), and Post waits until it matches what was typed. Ctrl or Cmd with Enter in the note
posts. A filed report is confirmed with a toast linking to it on GitHub.

## Roadmap

- More surfaces: meeting processing failures, mail account errors, failed agent turns, and the fault center the vision
  describes, each a subject Elysium reads from its own records rather than `error`.
- Logs: the API's own recent log lines for the failing connector, redacted the same way, once logs are kept anywhere
  they can be read back.
