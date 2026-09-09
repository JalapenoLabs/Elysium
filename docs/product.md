# The product

Elysium is one self-hosted workspace instead of six subscriptions. This document is the
product argument and the surfaces it implies. [CLAUDE.md](../CLAUDE.md) holds the vision;
this is what it means for the screens.

## The thesis

Google Workspace and Microsoft Teams are several products stapled together and rented to you.
The storage is a bolt-on, the management console is bad, every tool implements teams and
permissions separately, and none of it runs on a machine you own.

Elysium inverts each of those, and the inversion is the product:

| Constraint | What Elysium does instead |
|---|---|
| Someone else's compute | Runs on your machine; SSL and exposure are yours, everything else is handled |
| Six products, six models of a team | One identity, one tenancy, one permission model, one UI |
| Storage as a bolt-on | Storage is the substrate every other pillar persists onto |
| Rented, per seat, forever | Open source, self-hosted, capacity is whatever disk you point it at |
| Rebuilt tools you must migrate to | Run the managed service, or link the one you already use, or both at once |
| Hunt for what happened while you were away | Everything is brought to you in one action center |
| AI bolted onto a chat box | Elysia is the primary screen, and the tools are hers as much as yours |

The last two are the ones people will not ask for and will not want to give up.

## The surfaces

None of this is built. It is the shape the design implies, written down so the build order is
a decision rather than a drift.

### The workspace

The application root: a dockable rc-dock canvas with a bottom taskbar as its launcher. Panels
split, float, maximize, and pop out to real windows. Open the drive on the left and keep
Elysia on the right, or put the action center on a second monitor.

The framework's own screens keep the stamped shell and are reached from the taskbar. Elysium
does not redraw them. See [workspace.md](workspace.md) and
[navigation.md](navigation.md).

### The Elysia session

The flagship panel, and not a chat transcript. A status-colored ambient field (orange
running, red errored, pink needs you, green done), an icosahedron selector to rotate between
concurrent sessions, voice reporting, a sound grammar, and the full agent surface around the
conversation: plans, questions, usage, cost, incidents, artifacts.

Its data comes from a satellite, and everything about that half is
[satellites.md](satellites.md).

### The action center

The messaging inversion. Email and chat in one stream, because the split is an implementation
detail of the last thirty years and not a thing a person cares about. Items are **resolved**,
not read, and resolving one moves you to the next, with an auto-pause so you are never yanked
out of something mid-sentence. `ctrl+P` navigates to anything: a person, a message, a file, a
repository.

AI drafts replies without being asked, presented as options rather than sent. Elysia holds
context across threads, so a status request gets a draft that knows what actually happened.

### Drive

One keyboard-driven view over many Drives, live rather than synced. Search is federated and
streaming: results arrive from every provider as they come rather than after the slowest one.
See [storage.md](storage.md).

### Services

Git, mail, registry, and Perforce, each either a managed container Elysium provisions or an
account you linked, aggregated into one surface. "All my repos" spans a managed Forgejo and a
linked GitHub at once. Provisioning is admin-only; personal linking is gated by per-kind
policy. See [integrations.md](integrations.md) and [mail.md](mail.md).

### The fault center

Every failure, agent and application alike, with a stable code, in one place. Not a debug
panel: a first-class surface, because a control plane that hides failures is worse than one
with none to show.

## Build order

Each step is useful on its own. The tenancy tier gates most of them, which is why it is first
and why it is being built in the framework.

1. **The project tier**, in Anubis. Everything below owns resources, and a resource needs a
   tenant to belong to. [Anubis #93](https://github.com/JalapenoLabs/Anubis/issues/93).
2. **The dock shell**, with the framework's screens reachable from it. Unblocked, since a
   frame owns no resources.
3. **Satellites and the Elysia session**, through the live thread view. The first thing that
   feels like the product, and the reason the workspace exists.
4. **Drive**, the substrate every later pillar persists onto.
5. **Integrations and the provisioner**, then git on top of them.
6. **Mail**, which is the largest single pillar and needs Drive and the provisioner first.
7. **The action center**, which needs mail and chat to have something to unify.

## What Elysium will not do

- **Re-implement git, SMTP, a registry, or Perforce.** It provisions proven implementations
  and owns the unification. A hand-rolled SMTP server is a security incident with a roadmap.
- **Mirror files.** The provider is the source of truth. Sync is a later opt-in feature, never
  the core engine.
- **Enforce what a satellite enforces.** Elysium stores and forwards agent permissions. The
  wall is on the satellite; a second wall drawn here would be scenery.
- **Hide failures.** Filtering happens after they are stored and shown, never before.
- **Put a credential in a browser.** Not a satellite secret, not a Drive credential, not a
  Docker socket. Not in a form response, not in a debug panel, not once.
- **Require a Kubernetes cluster.** Docker first, Kubernetes as a later opt-in backend.
  Requiring a cluster would gut the "runs on a home machine" premise.
