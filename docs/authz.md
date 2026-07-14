# Authorization (org RBAC)

How the API decides whether an authenticated caller may perform an org-scoped
action. This is distinct from authentication (who the caller is, see
`auth-model.md`) and from `org_auth_policies` (which sign-in *methods* an org
allows, also authn). Here the question is: given a signed-in caller and a target
org, are they allowed to do this?

## Model

Membership and role live in `org_members` (`user_id`, `org_id`, `role`). The role
is one of three values, totally ordered by privilege:

```
owner > admin > member
```

A profile can belong to many orgs with a different role in each, so authorization
is always evaluated against a specific target org, never globally.

## The guard

`backend/src/authz.rs` is the single place roles are interpreted:

- `OrgRole` mirrors the stored values and derives `Ord`, so "at least admin" is
  the comparison `role >= OrgRole::Admin`.
- `role_in_org(pool, user_id, org_id)` resolves the caller's role in one org, or
  `None` when they are not a member.
- `require_org_role(state, principal, org_id, minimum)` is the handler gate: it
  resolves the session principal to a user, checks membership and rank, and on
  failure returns a ready HTTP response the handler returns as-is.

Non-membership and insufficient role both return **403**, never 404, so a
non-member cannot use these endpoints to discover that an org exists. A handler
that already resolved the caller's id (e.g. to write an audit row) calls
`role_in_org` directly to avoid resolving twice; everything else uses
`require_org_role`.

## Where it is enforced

Every org-privileged mutation runs through the guard:

- `PATCH /api/v1/orgs/:id` (rename / logo) requires **admin**.
- `POST /api/v1/auth/invites` (mint an invite) requires **admin**, and the
  invited role may not outrank the inviter, so an admin cannot mint an owner
  invite and escalate.

Routes that are not gated are so by design: self-scoped reads and edits (`/me`,
`/users/me`, `/context`, status), session-scoped switches that already check
membership (`/session/switch-org`, `/session/accounts`), open-by-intent routes
(`POST /orgs` creates your own org; invite `preview` is for the invitee), and
token-gated invite acceptance.

## Roadmap

- Org member management (list members, change a member's role, remove a member),
  each gated on the guard, with the same no-escalation rule on role changes.
- Team-level roles and permissions (`team_members` exists; no management flow
  yet).
- The access-denied surface: the client renders its `/denied` route on a 403.
- Enforcing `org_auth_policies` at sign-in (disabled-method rejection, org-wide
  mandatory TOTP, session lifetime), which is policy enforcement adjacent to this
  RBAC layer.
