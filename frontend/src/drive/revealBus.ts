// Copyright © 2026 Jalapeno Labs

// A tiny seam letting any surface (search results, future notifications) send
// the Files panel to a location. Same queue-until-bound pattern as the
// workspace controller: reveals requested before the panel mounts are
// delivered on bind.
export type RevealTarget = {
  driveId: string
  // The folder path to open, relative to the Drive root.
  path: string
}

let listener: ((target: RevealTarget) => void) | null = null
let pending: RevealTarget | null = null

export function revealInFiles(target: RevealTarget) {
  if (listener) {
    listener(target)
  }
  else {
    pending = target
  }
}

export function bindRevealListener(next: (target: RevealTarget) => void): () => void {
  listener = next
  if (pending) {
    next(pending)
    pending = null
  }

  return () => {
    if (listener === next) {
      listener = null
    }
  }
}
