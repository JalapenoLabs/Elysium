// Copyright © 2026 Jalapeno Labs

// A tiny reactive registry: contributors register keyed entry lists, consumers
// read one flattened snapshot and subscribe for changes. This is the seam that
// lets pillars plug into shared surfaces (the command palette, the workspace
// panel roster) without those surfaces knowing any pillar exists. The snapshot
// is referentially stable between changes, so it drops straight into
// useSyncExternalStore.
export function createRegistry<Entry>() {
  const entriesByKey = new Map<string, Entry[]>()
  const listeners = new Set<() => void>()
  let snapshot: Entry[] = []

  function rebuildSnapshot() {
    snapshot = [ ...entriesByKey.values() ].flat()
    for (const listener of listeners) {
      listener()
    }
  }

  function register(key: string, entries: Entry[]): () => void {
    entriesByKey.set(key, entries)
    rebuildSnapshot()

    return () => {
      // Only clear the key if this registration is still the active one, so a
      // re-register followed by the old unregister cannot wipe the new entries.
      if (entriesByKey.get(key) === entries) {
        entriesByKey.delete(key)
        rebuildSnapshot()
      }
    }
  }

  function list(): Entry[] {
    return snapshot
  }

  function subscribe(listener: () => void): () => void {
    listeners.add(listener)
    return () => {
      listeners.delete(listener)
    }
  }

  return {
    register,
    list,
    subscribe,
  } as const
}
