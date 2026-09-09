# Tenancy: org, project, team, individual

Elysium's tenancy has four tiers. Resources and work live in **projects**; the organization
is a thin umbrella. This layer is foundational: it decides resource ownership across storage
([storage.md](storage.md)), integrations ([integrations.md](integrations.md)), and mail
([mail.md](mail.md)).

**This tier does not exist in Anubis yet, and Elysium's domain model is blocked on it.**
Anubis owns tenancy, so the project tier is being built in the framework rather than here.
See [Anubis #93](https://github.com/JalapenoLabs/Anubis/issues/93) and
[anubis.md](anubis.md). What follows is the design the framework is being asked to own.

## The tiers

- **Organization**: a thin umbrella. A logo, membership, org-level teams and roles,
  cross-project administration, and billing. Being a **full organization member grants
  access to every project in it.** The organization owns almost no work resources
  directly; it owns people and policy.
- **Project**: the sub-tenant workspace, the GCP-project and Jira-project analog. A project
  **owns its domains, its Drives, its service integrations (mail, git, registry, Astraval),
  its teams, and its membership and roles.** It is the real unit of work and the unit of
  container isolation.
- **Team**: a group with a role, either org-level or project-level. An org-level team is
  **inherited by every project**; a project-level team is scoped to that one. Same shape as
  GitHub org teams against repository access.
- **Individual**: a person. A **full organization member** reaches all projects. A **guest**
  is not a full member and is scoped to specific projects or teams only.

## Rules

- **Every organization has at least one project.** Creating one auto-creates a **default
  project**, and the auto-provisioned default Drive lands there. A simple organization may
  never think about projects; a power user adds more.
- **Organization membership cascades to all projects** for full members. Guests do not
  cascade; they are added to a project or team explicitly.
- **Org-level teams are inherited by all projects**; project-level teams are local.
- **Organization admins and owners** get cross-project visibility through the umbrella, the
  same admin-bypass pattern Drive access uses.

## Roles per tier

- **Organization**: owner, admin, member, guest.
- **Project**: owner, admin, member. A project owner controls its resources, integrations,
  domains, and membership.
- **Team**: owner, admin, member.

Effective permission on a resource resolves through whichever tier grants it: organization
admin bypass, then project role, then team role, then a direct individual grant.

**One function owns that resolution**, returning the strongest role a user holds on a
project across every tier: an organization owner or admin bypasses to owner; a full member
cascades to member; a guest cascades to nothing and is scoped to explicit grants; an
org-level team is inherited by every project while a project-scoped team never leaks; and a
suspended member is cut entirely. Resource authorization resolves through it or through the
guard built on it, never by re-deriving access. Re-deriving is how two call sites disagree
about who can read a file.

## Resource ownership

Work resources are **project-scoped**. An owner scope is `org`, `project`, or `personal`:

- **Drives** carry a project; the project owns its storage locations.
- **Integrations** (mail, git, registry, Astraval) are project-scoped by default. The mail
  server in particular is a **per-project** managed integration bound to the project's
  domains.
- A few resources are **org-shared** across projects, or **personal** to one member's own
  linked account. The default home for work is the project.

## Runtime: shared control plane, per-project data plane

One machine hosts many organizations, each with many projects. The split:

- **Shared control plane**: one Postgres, one API, one reverse proxy, one SMTP router.
- **Per-project data plane**: each project's services run in their own containers, fully
  isolated, so nothing conflicts across projects.

```
Org: JalapenoLabs
  Project Yearloom.io      -> domains + Drives + mail container
  Project EmailPilot       -> domains + Drives + mail container
  Project JalapenoLabs.io  -> domains + Drives + mail container
Org: NavarroCity
  Project NavarroCity.com  -> domains + Drives + mail container
```

Four projects, four isolated mail containers, one shared API, database, and router.

## Data model

- `projects (id, organization_id, name, slug, created_at, ...)`, owning its domains.
- `project_members (project_id, user_id, role)`: explicit membership and role. Full
  organization members are granted implicitly, guests explicitly.
- `teams` gains a nullable `project_id`. **NULL is org-level and inherited by every
  project**; set is project-scoped.
- `drives` carry a **NOT NULL** project, because every organization always has a default
  project, so a Drive always has a project home.
- `integrations` carry a nullable project alongside their owner scope, since an integration
  may be org-shared or personal.

## Management surface

Projects get the CRUD, membership, and domain surface every tenant tier gets:

- **List** returns the projects the caller can reach in their active organization: every
  project for a full member, only the explicitly granted ones for a guest. The set is
  filtered through the resolver, never re-derived in SQL.
- **Create** requires `manage_projects`. The named owner, or the creator when unset, becomes
  its owner. An omitted slug is derived from the name: the clean URL-safe base when free
  within the organization, with a numeric suffix appended only on an actual unique-index
  collision, retried, so a concurrent create races safely. An explicit slug is used verbatim
  and a collision answers 409.
- **Rename and re-slug** is open to a project owner or admin, or a `manage_projects` holder.
  An empty field is left unchanged.
- **Delete** requires `manage_projects` and is refused with 409 when it is the organization's
  last project. The count and the delete are one atomic statement, so concurrent deletes can
  never race an organization down to zero projects.
- **Roster** management belongs to a project's own owners and admins. To prevent escalation
  an actor may not grant a role above their own, nor remove someone who outranks them.
  Viewing the roster needs project membership.
- **Domains** attach and detach under `manage_projects`. A domain maps to exactly one
  project across the deployment, so attaching one already owned answers 409.

`manage_projects` is delegatable through the organization's grants, exactly as
`manage_teams` is; owners and admins hold it implicitly. A project's own membership is
managed by its owners and admins, so day-to-day project administration never needs an
organization-level grant.

## Configurability

Following admin.google.com: plan for deep, granular, per-tier configuration with sane
defaults. Org-wide policies, project-level overrides, and team and role scoping are all
expected to be richly configurable rather than hard-coded.
