// Copyright © 2026 Jalapeno Labs

import type { DEFAULT_LOCALE, DEFAULT_NAMESPACE, resources } from '../i18n'

// Makes every `t()` key type-checked against the en-US source files. Module
// augmentation only merges into interfaces, so this cannot be a `type`.
declare module 'i18next' {
  interface CustomTypeOptions {
    defaultNS: typeof DEFAULT_NAMESPACE
    resources: (typeof resources)[typeof DEFAULT_LOCALE]
  }
}
