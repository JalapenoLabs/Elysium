// Copyright © 2026 Jalapeno Labs

//! What every Studio thread's agent is told, below the satellite's own instructions.
//!
//! A code constant, like the rest of Elysium's thread policy. The fixed render set is what
//! lets the filmstrip compare one turn with the next, and the `-hero` render is what the
//! grid shows (see `super::thumbnail`), so both are spelled out rather than left to taste.
//! See `docs/studio.md`, What agents are told.

/// Written into the agent's `AGENTS.md` for every Studio thread.
pub const INSTRUCTIONS: &str = "\
# Studio

You are making one asset for Elysium Studio: a 3D model, a render, or a 2D image. There are no
repositories in this workspace.

## Deliverables go in artifacts/

- Put only finished deliverables in the workspace's `artifacts/` directory. Elysium keeps every
  file that appears or changes there after each turn and shows it to the user. Scratch files,
  test renders, and downloads stay outside it.
- One asset per item. Name its files after it with a short lowercase stem, such as `banana`.

## 3D work

- Model in Blender through the `blender` MCP server. Save the source as
  `artifacts/<stem>.blend` before the turn ends; the Blender scene does not outlive the turn.
- Do not export glTF yourself. Elysium exports `artifacts/<stem>.glb` from the `.blend` after
  every turn.
- After any turn that changes how the asset looks, render four views to
  `artifacts/renders/<stem>-<view>.png`: `hero` (a three-quarter view), `front`, `side`, and
  `top`. Render each at 1024 by 1024 pixels on a neutral studio backdrop with three-point
  lighting, framed so the whole asset is visible. Keep the same framing from turn to turn so
  the user can compare them.

## 2D work

- Write the image to `artifacts/<stem>.png`, and also `artifacts/<stem>.svg` when vector art
  suits it.
- When a presentation mockup helps, add `artifacts/<stem>-hero.png`.

## Feedback

An attached image is the user's drawing over one of your renders or over a view of the model.
The strokes mark what to change; the prompt says how. Look at it closely before you start.
";
