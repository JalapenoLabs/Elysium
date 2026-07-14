# Auth Art & Asset Briefs

Hand-off briefs for the visual and audio assets the auth surface needs. Each is written so it
can go straight to a designer, a motion artist, a sound designer, or an asset marketplace. The
design intent behind these assets lives in `auth.md` (the cinematic layer); this document is the
procurement list.

Shared direction for everything below: **futuristic, artistic, sci-fi** with cinematic
game-title energy (huge type, staged transitions, a world that reacts), while staying premium,
never loud or gamer-neon. It must read as a product that has existed for years, not a launch demo.
Everything ships in both **dark and light** themes and must respect `prefers-reduced-motion`.

## 1. Signature ambient background (the hero asset)

The single most important asset. It is the living field behind every auth screen and sets the
tone for the entire product.

- **Form:** a looping, slow-moving abstract field. A GPU shader (WebGL/GLSL) or a seamless video
  loop. Prefer a shader so it can be driven by state and stays crisp at any resolution.
- **State-driven color:** must accept a state input and shift its dominant hue: calm
  idle (deep indigo/cyan), **orange** authenticating, **pink** needs-input, **red** error,
  **green** success. Transitions between states are smooth, over ~600ms.
- **Motion:** ambient drift only; nothing that competes with a form for attention. Must be
  legible behind text at all times (built-in contrast scrim).
- **Deliverables:** the shader/source, a documented `state` parameter, a static poster frame per
  state per theme (for reduced-motion and first paint), and a low-cost mobile fallback.
- **Budget note:** worth real investment. This is the face of the app.

## 2. Elysia presence geometry

The ambient figure that reacts to the flow. A nod to the workspace icosahedron, not a mascot.

- **Form:** a parametric 3D object (or a Lottie/rig set) we can drive from code, with named
  states: idle, thinking (pulse), needs-input (lean-in), success (settle), error (recoil).
- **Style:** faceted, translucent, luminous edges; feels alive without eyes or a face.
- **Deliverables:** the rig or model plus a state API, and a lightweight 2D fallback (Lottie or
  sprite) for low-power devices.

## 3. Logo, wordmark, and app iconography

- **Lockups:** Elysium (product) and Elysia (the AI) wordmarks, horizontal and stacked, in dark
  and light.
- **App icons:** favicon set, PWA/maskable icons, Electron app icons (macOS `.icns`, Windows
  `.ico`, Linux PNGs).
- **Social/OG cards:** share images for sign-in and invite links.
- **Deliverables:** SVG masters plus exported sizes, with a minimum clear-space and color spec.

## 4. Sound pack

Short, cohesive, sci-fi, never annoying. All gated behind a mute preference.

- **Cues:** success chime, error tone, "needs-input / your turn" prompt (plays when the passkey
  challenge appears; doubles as an accessibility signal), and a couple of subtle UI ticks.
- **Style:** soft, warm, spacious; a family that clearly belongs together.
- **Deliverables:** normalized loudness, short (<1.5s) files in a web-friendly format, both a
  full and a "subtle" set.

## 5. Fault-state illustrations

So even the edges feel intentional. One illustration each for: invalid/expired invite, access
denied, account suspended, and identity-unavailable.

- **Style:** consistent with the ambient/sci-fi direction; quiet and reassuring, not comedic.
- **Deliverables:** SVG or high-res PNG, dark and light variants.

## 6. Elysia voice (optional, opt-in)

Only if we pursue spoken feedback.

- **Lines (v1):** a brief "Welcome back" on success; optionally a gentle "Tap your passkey."
- **Character:** calm, warm, confident; neither robotic nor overly casual.
- **Deliverables:** either a chosen TTS voice profile and parameters, or recorded lines with a
  documented voice spec so future lines stay consistent.

## Priority order

1. Signature ambient background (#1) — blocks the whole cinematic feel.
2. Logo/wordmark/icons (#3) — needed early for every screen.
3. Elysia presence (#2) — high impact, can land just after the flows work.
4. Sound pack (#4) and fault illustrations (#5) — polish pass.
5. Voice (#6) — last, and only if we commit to it.
</content>
