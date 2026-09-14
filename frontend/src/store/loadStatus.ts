// Copyright © 2026 Jalapeno Labs

// Where a server-backed collection stands. `loaded` stays set while a refetch runs,
// so a live list never flashes back to a spinner when the event stream reconnects.
export type LoadStatus = 'idle' | 'loading' | 'loaded' | 'failed'
