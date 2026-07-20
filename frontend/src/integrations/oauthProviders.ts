// Copyright © 2026 Jalapeno Labs

import type { IconType } from 'react-icons'

// Misc
import { FaGithub, FaGoogle, FaMicrosoft } from 'react-icons/fa6'
import { LuPlug } from 'react-icons/lu'

// Display metadata for the OAuth providers Elysium can link (#35). The server
// is the source of truth for which providers exist and are configured; this
// only maps a provider id to its brand icon. An unknown provider falls back to
// a generic plug so a newly added backend provider still renders.
const iconByProvider: Record<string, IconType> = {
  github: FaGithub,
  google: FaGoogle,
  microsoft: FaMicrosoft,
}

export function providerIcon(provider: string): IconType {
  return iconByProvider[provider] ?? LuPlug
}
