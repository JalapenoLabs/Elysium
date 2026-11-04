// Copyright © 2026 Jalapeno Labs

// Core
import { defineConfig, globalIgnores } from 'eslint/config'
import cliBaseConfig from '@jalapenolabs/cli/eslint'

// Plugins
import react from 'eslint-plugin-react'
import reactHooks from 'eslint-plugin-react-hooks'

// The `Intl` formatters only `src/format/` builds, and what lint points to instead.
const INTL_FORMATTERS = '/^(DateTimeFormat|NumberFormat|RelativeTimeFormat|DurationFormat)$/'
const USE_FORMAT_MODULE = 'Format dates and numbers with src/format/ (formatDate, formatNumber, ...).'

// The org-wide Jalapeno Labs ruleset, with only the overrides this app needs.
export default defineConfig([
  {
    extends: [ cliBaseConfig ],
  },
  globalIgnores([
    '.yarn/**',
    'dist/**',
  ]),
  {
    // @jalapenolabs/cli@e3f02f8 intends to apply react/recommended and
    // react-hooks/recommended to TSX, but passes fixupConfigRules' array result
    // along as if it were one config, so none of those rules reach any file.
    // Restore them here until the shared config is fixed, keeping its overrides.
    // The plugins themselves are already registered by the shared TSX block.
    files: [ '**/*.{tsx,jsx}' ],
    rules: {
      ...react.configs.flat.recommended.rules,
      ...reactHooks.configs.recommended.rules,
      'react/react-in-jsx-scope': 'off',
      'react-hooks/exhaustive-deps': 'off',
    },
  },
  {
    // Every toast goes through src/toast/toast.tsx, which times it by length, keeps errors up
    // until dismissed, and records it for the notification menu. HeroUI's own `toast` would
    // skip all of that, so only the wrapper may import it.
    files: [ '**/*.{ts,tsx}' ],
    ignores: [ 'src/toast/toast.tsx' ],
    rules: {
      'no-restricted-imports': [
        'error',
        {
          paths: [
            {
              name: '@heroui/react',
              importNames: [ 'toast' ],
              message: 'Import toast from src/toast/toast.tsx, which applies Elysium\'s toast rules.',
            },
          ],
        },
      ],
    },
  },
  {
    // Dates and numbers are formatted by `src/format/` alone, so every one in the app reads the
    // same way and in the viewer's zone (docs/frontend.md, Dates and numbers).
    files: [ '**/*.{ts,tsx}' ],
    ignores: [ 'src/format/**' ],
    rules: {
      'no-restricted-syntax': [
        'error',
        {
          selector: `NewExpression[callee.object.name="Intl"][callee.property.name=${INTL_FORMATTERS}]`,
          message: USE_FORMAT_MODULE,
        },
        {
          selector: `CallExpression[callee.object.name="Intl"][callee.property.name=${INTL_FORMATTERS}]`,
          message: USE_FORMAT_MODULE,
        },
        {
          selector: 'CallExpression[callee.property.name=/^toLocale(Date|Time)?String$/]',
          message: USE_FORMAT_MODULE,
        },
      ],
    },
  },
  {
    // The shared config spells the owner "JalapenoLabs"; this repository's
    // headers use the company name, matching @jalapenolabs/uikit.
    files: [ '**/*.{js,ts,tsx}' ],
    rules: {
      'license-header/header': [
        'error',
        [
          `// Copyright © ${new Date().getFullYear()} Jalapeno Labs`,
        ],
      ],
    },
  },
])
