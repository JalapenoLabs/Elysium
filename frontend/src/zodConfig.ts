// Copyright © 2026 Jalapeno Labs

// Utility
import { z } from 'zod'

// Zod compiles validators with `new Function` when it can, and finds out by trying once, as the
// first object schema is built. The page's Content Security Policy forbids eval, so that probe would
// be reported as a violation on every load; jitless skips it. A module of its own, imported first by
// `main.tsx`, since a schema may be built as any other module loads.
z.config({ jitless: true })
