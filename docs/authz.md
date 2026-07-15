# Authorization (org RBAC)

How the API decides whether an authenticated caller may perform an org-scoped
action. This is distinct from authentication (who the caller is, see
`auth-model.md`) and from `org_auth_policies` (which sign-in *methods* an org
allows, also authn). Here the question is: given a signed-in caller and a target
org, are they allowed to do this?

## Model

Membership and role live in `org_members` (`user_id`, `org_id`, `role`,
`suspended_at`). The role is one of four values, totally ordered by privilege:

```
owner > admin > member > guest
```

A profile can belong to many orgs with a different role in each, so authorization
is always evaluated against a specific target org, never globally. A **suspended**
member (`suspended_at` set) keeps their row but has no effective access: the guard
treats them as a non-member and realtime fan-out excludes them.

### Delegated capabilities

An org can grant a *member* specific admin powers without full admin, recorded in
`org_admin_grants`: `manage_members`, `manage_security`, `manage_teams`,
`view_audit`. Owners and admins hold every capability implicitly; a member holds
one only if it was granted; guests and suspended members hold none. Managing the
grants themselves requires being a real owner/admin, so delegation never cascades.
There is deliberately no billing capability, the project gates nothing.

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

- `PATCH /api/v1/orgs/:id` (rename / logo / branding) requires **admin**.
- `POST /api/v1/auth/invites` (mint an invite) requires **admin**, and the
  invited role may not outrank the inviter, so an admin cannot mint an owner
  invite and escalate.
- **Member management** (`.../orgs/:id/members`): any member may view the roster;
  changing a role or removing a member requires **admin**, the actor must outrank
  the target, and no new role may exceed the actor's own. Owners are therefore
  never demoted or removed here and the last owner is always safe.
- **Teams** (`.../orgs/:id/teams`, `.../teams/:id`): creating, renaming, deleting a
  team and managing its members require org **admin**; viewing is open to any
  member. A team member must already belong to the team's org.

Routes that are not gated are so by design: self-scoped reads and edits (`/me`,
`/users/me`, `/context`, status), session-scoped switches that already check
membership (`/session/switch-org`, `/session/accounts`), open-by-intent routes
(`POST /orgs` creates your own org; invite `preview` is for the invitee), and
token-gated invite acceptance.

## Roadmap

- Team-scoped permissions: today a team's own roles are stored but not yet used to
  gate anything (org role is the gate); wire team roles to team-scoped actions.
- Ownership transfer and self-service "leave an org" (member management
  deliberately forbids self-management and touching owners).
- The access-denied surface: the client renders its `/denied` route on a 403.
- Remaining `org_auth_policies` enforcement (org-wide mandatory TOTP for members
  with none, `session_lifetime_seconds`); the password disabled-method rejection is
  already enforced (see `security.md`).
