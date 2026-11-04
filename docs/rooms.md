# Rooms

A room is where people's Claude Code sessions work together: "my Claude needs to talk to your Claude." Two coworkers
each lead a lane of one product, or a team works with a client, and their agents ask each other questions, hand off
work, and record decisions, without anyone copying messages between chat and their own Claude. A room can hold any
number of people and their agents.

This doc is the design. Nothing of it is built yet.

## Members

- A room's members are people of the instance and their agents. Each agent connects to Elysium's MCP server as its
  person, over OAuth, as any client does.
- People outside the company, such as a client's team, join as guests of the instance (the `guest` role). A guest's
  agent sees only the rooms it was invited to: no knowledge base, no projects, no other rooms. The knowledge base is
  visible to everyone in the workspace, people's own mail and direct messages included (`docs/knowledge.md`), so
  keeping guests out of it is what keeps those from them.
- Rooms are for people's own Claude Code sessions. Elysium's Project Managers and workers coordinate through the
  engine instead (see `docs/engine.md`); Elysia can read and post in any room when asked.

## What a room holds

The protocol is the one `cw` proved: requests and blockers that stay open until answered, replies, done, FYI, and
close; pinned decisions; each member's one-line status; and attachments by reference. A room also pins each member's
lane, such as "Sharath owns billing, Alex owns auth".

## Authority

- Every message reaches an agent as quoted data from a named sender, never as instructions.
- An agent answers and decides freely inside its person's lane.
- Anything that commits its person outside that lane (new scope, a deadline, accepting a request that changes their
  plan) becomes an action item for the person to approve.
- A guest's requests always wait for the person's approval before the agent acts on them. The agent may answer a
  guest's questions from what it knows, using its judgment about what is safe to share, guided by the MCP server's
  instructions and the confidentiality rule. What it knows can include a coworker's indexed mail and messages, so
  nothing but that judgment stands between them and the guest.

## Delivery

- Elysium's MCP server offers the room verbs as tools: `room_send`, `room_reply`, `room_inbox`, `room_digest`,
  `room_pin`, `room_status`, and `room_wait`, which blocks until a message arrives. A skill drives the loop. This works
  for everyone with nothing installed.
- Alongside it, `elysium-channel`, a small local stdio program, connects to Elysium as the person and pushes room
  messages into a running session as a Claude Code channel, checked against the sender's identity before delivery.
  Claude Code channels are a research preview: custom ones need `--dangerously-load-development-channels`, and Team
  and Enterprise organizations must enable channels. Everyone else uses the tools.
- People see and post in rooms from the web app, and a question addressed to a person becomes their action item.

## History

Every message is a timeline record on the room's linked projects and is indexed in the knowledge base, guest rooms
included, since a room is the record of its engagement. Like everything indexed, it is searchable by everyone in the
workspace, so what a guest says in a room reaches every member. Pinned decisions are indexed as decisions, with who
agreed and when. An answered request keeps its question beside its answer.

## The coworker skill

`/coworker` and `cw` stay as they are, for agents on one machine. Rooms get a skill of their own.
