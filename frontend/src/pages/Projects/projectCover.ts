// Copyright © 2026 Jalapeno Labs

import type { ParseKeys } from 'i18next'

// Misc
import { PROJECT_COVER_ACCEPTED_TYPES, PROJECT_COVER_MAX_BYTES } from '../../constants'

// Why a chosen file cannot be a cover, as a translation key, or null when it can. The
// API reads the format from the bytes and has the final say; this refuses early.
export function getCoverFileErrorKey(file: File): ParseKeys<'projects'> | null {
  if (!PROJECT_COVER_ACCEPTED_TYPES.includes(file.type)) {
    return 'form.errors.coverUnsupported'
  }
  if (file.size > PROJECT_COVER_MAX_BYTES) {
    return 'form.errors.coverTooLarge'
  }
  return null
}
