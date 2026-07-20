# Projects & the org → project → team hierarchy

Elysium's tenancy has four tiers: **Org → Project → Team → Individual**. Resources and
work live in **projects**; the org is a thin umbrella. This layer is foundational — it
reshapes resource ownership across storage (`docs/storage.md`) and integrations
(`docs/integrations.md`), and it extends the identity model in `docs/identity.md`.

## The tiers

- **Org** — a thin umbrella: a logo, membership, org-level teams/roles, cross-project
  admin, and (eventually) billing. Being a **full org member grants access to every
  project in the org.** The org owns almost no work resources directly; it owns *people
  and policy*.
- **Project** — a sub-tenant workspace (the GCP-project / Jira-project analog): Yearloom.io,
  EmailPilot, NavarroCity.com. A project **owns its domain(s), its Drives, its service
  integrations (mail/git/registry/Astraval), its teams, and its membership + roles.** It is
  the real unit of work and the unit of **container isolation** (see Runtime below).
- **Team** — a group with a role, **org-level or project-level**. An org-level team is
  **inherited by every project**; a project-level team is scoped to that one project.
  (Same shape as GitHub org-teams vs repo access.)
- **Individual** — a person. A **full org member** reaches all projects. A **guest /
  restricted** individual (the existing Guest role) is *not* a full org member and is scoped
  to specific project(s)/team(s) only — the "minimal resource" case.

## Rules

- **Every org has at least one project.** Creating an org auto-creates a **default project**;
  the auto-provisioned default Drive and first workspace land there. Simple orgs may never
  think about projects; power users add more.
- **Org membership cascades to all projects** (full members). Guests do not cascade — they
  are added explicitly to a project/team.
- **Org-level teams are inherited by all projects**; project-level teams are local.
- **Org admins/owners** get cross-project visibility via the umbrella (admin bypass, same
  pattern as Drive access).

## Roles per tier

- **Org:** owner / admin / member / guest (already built).
- **Project:** owner / admin / member (projects have owners — a project owner controls its
  resources, integrations, domains, and membership).
- **Team:** owner / admin / member (already built).

Effective permission on a resource resolves through whichever tier grants it: org admin
bypass, then project role, then team role, then direct individual grant.

`backend/src/projects.rs::resolve_project_access` is the single source of truth for this
resolution. It returns the strongest `ProjectRole` a user holds on a project across every
tier: an org owner/admin bypasses to owner; a full org member cascades to member; a guest
cascades to nothing and is scoped to explicit project/team grants; an org-level team
(`teams.project_id IS NULL`) is inherited by every project while a project-scoped team
never leaks; and a suspended org member is cut entirely. Resource authz (storage,
integrations, mail) resolves through it, `has_project_access` (the role-threshold `can`
check), or the `require_project_role` HTTP guard, never by re-deriving access.

## Resource ownership

Work resources are **project-scoped**. Concretely, `owner_scope` becomes **`org | project |
personal`** (extending the `org | personal` in `docs/integrations.md`):

- **Drives** carry a `project_id`; the project owns its storage locations.
- **Integrations** (mail/git/registry/Astraval) are project-scoped by default; the mail
  server, in particular, is a **per-project** managed integration bound to the project's
  domain(s).
- A few resources may still be **org-shared** (span projects) or **personal** (a member's
  own linked account) — but the default home for work resources is the project.

## Runtime: shared control plane, per-project data plane

One machine can host many orgs, each with many projects. The split:

- **Shared control plane:** one Postgres, one API, one reverse proxy, one SMTP router.
- **Per-project data plane:** each project's services (mail, and later workspace services)
  run in their **own containers**, fully isolated — nothing conflicts across projects.

Example on one self-hosted machine:

```
Org: JalapenoLabs
  Project Yearloom.io      → domain + Drives + mail container + ...
  Project EmailPilot       → domain + Drives + mail container + ...
  Project JalapenoLabs.io  → domain + Drives + mail container + ...
Org: NavarroCity
  Project NavarroCity.com  → domain + Drives + mail container + ...
```

Four projects, four isolated mail containers, one shared API/DB/router.

## Data model (shape)

- `projects (id, org_id, name, slug, domain(s), created_at, ...)` — a project belongs to an
  org; owns domains.
- `project_members (project_id, user_id, role)` — explicit project membership + role;
  full org members are granted implicitly, guests explicitly.
- `teams` gains a nullable `project_id` — **NULL = org-level (inherited by all projects)**,
  set = project-scoped.
- `drives` gain a **NOT NULL** `project_id` (every org always has a default project, so a
  Drive always has a project home); `integrations` gain a nullable `project_id` (with
  `owner_scope`, since an integration may be org-shared or personal), so resources
  attribute to a project.

## Management API

`backend/src/projects.rs` exposes the protobuf CRUD, membership, and domain surface
under `/api/v1/projects` (wired in `backend/src/api.rs`). The collection is scoped to
the caller's active org; item routes carry the project id.

- `GET /projects` lists the projects the caller can reach in their active org: every
  project for a full org member, only the explicitly-granted ones for a guest. The
  set is filtered through `resolve_project_access`, never re-derived in SQL.
- `POST /projects` creates a project (requires `manage_projects`). The named owner, or
  the creator when unset, becomes its OWNER; the slug is derived from the name when
  omitted.
- `PATCH /projects/{id}` renames or re-slugs a project (a project owner/admin, or a
  `manage_projects` holder). An empty field is left unchanged.
- `DELETE /projects/{id}` deletes a project (requires `manage_projects`), refused with
  `409` when it is the org's last project. The count-and-delete is one atomic
  statement, so concurrent deletes can never race an org down to zero projects.
- `GET /projects/{id}/members`, `PUT /projects/{id}/members/{userId}`,
  `DELETE /projects/{id}/members/{userId}` manage the roster. A project owner/admin
  mutates membership; to prevent escalation the actor may not grant a role above
  their own or remove someone who outranks them. Viewing the roster needs project
  membership.
- `POST /projects/{id}/domains`, `DELETE /projects/{id}/domains/{domain}` attach and
  detach domains (require `manage_projects`). A domain maps to exactly one project
  across the deployment, so attaching one already owned is refused with `409`.

The **`manage_projects`** capability is delegatable through the org grants system
(`org_admin_grants`), exactly like `manage_teams`; owners and admins hold it
implicitly. A project's own membership, by contrast, is managed by its owners and
admins via `require_project_role`, so day-to-day project administration never needs
an org-level grant.

## Configurability

Per the admin.google.com principle: **plan for deep, granular, per-tier configuration with
sane defaults.** Org-wide policies, project-level overrides, and team/role scoping are all
expected to be richly configurable rather than hard-coded.

## Weaving into existing milestones

This layer is a prerequisite that touches issues already created:

- **Storage (M1):** `drives` gains `project_id`; the default Drive is provisioned into the
  default project; Drive access resolves through project membership.
- **Integrations (M2):** the integration data model gains `project` owner scope and
  `project_id`; the reconciler provisions per-project containers.
- **Identity/authz (M3):** `manage_integrations` and storage capabilities resolve against the
  org→project→team hierarchy; the permission model gains the project tier.

These are called out in the Projects milestone so the foundational change lands coherently
rather than being retrofitted per-issue.
