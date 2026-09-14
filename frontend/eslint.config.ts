// Copyright © 2026 Jalapeno Labs

// Core
import { defineConfig, globalIgnores } from 'eslint/config'
import cliBaseConfig from '@jalapenolabs/cli/eslint'

// Plugins
import react from 'eslint-plugin-react'
import reactHooks from 'eslint-plugin-react-hooks'

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
    // The shared config spells the owner "JalapenoLabs"; this repository's
    // headers use the company name, matching @jalapenolabs/uikit.
    files: [ '**/*.{ts,tsx}' ],
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
