// Copyright © 2026 Jalapeno Labs

// Core
import i18next from 'i18next'
import HttpBackend from 'i18next-http-backend'
import { initReactI18next } from 'react-i18next'

// Misc
import { parseJsonSafe } from '@jalapenolabs/uikit'

// Locale bundles are fetched at runtime from the static `/public/locales` folder,
// not imported at build time. Bundling every translation into the app JS would
// bloat the initial download as languages are added; the HTTP backend loads only
// the namespaces a session needs, on demand. Bundles are JSONC so translators can
// leave comments.
export const DEFAULT_LOCALE = 'en-US'

// `common` holds shared vocabulary (Save, Cancel, the product names); every
// feature namespace (here, `auth`) stays small and domain-scoped. `init` returns
// a promise, awaited in main.tsx so the first paint already has translations.
export const i18nReady = i18next
  .use(HttpBackend)
  .use(initReactI18next)
  .init({
    lng: DEFAULT_LOCALE,
    fallbackLng: DEFAULT_LOCALE,
    // Load only the exact locale (`en-US`), never the region-stripped variant.
    // i18next's default (`load: 'all'`) would ALSO request `/locales/en/*.jsonc`,
    // which doesn't exist; the SPA then serves index.html for it, and the JSONC
    // parser fails on the HTML. `currentOnly` avoids that phantom fetch entirely.
    load: 'currentOnly',
    ns: [ 'common', 'auth' ],
    defaultNS: 'common',
    backend: {
      loadPath: '/locales/{{lng}}/{{ns}}.jsonc',
      // Parse with the JSONC parser so comments in the bundles are tolerated.
      parse: (data: string) => parseJsonSafe(data) ?? {},
    },
    interpolation: {
      // React already escapes rendered values, so i18next must not double-escape.
      escapeValue: false,
    },
  })

export { i18next }
