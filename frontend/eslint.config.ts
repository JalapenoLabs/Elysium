// Copyright © 2026 Jalapeno Labs

import { defineConfig, globalIgnores } from 'eslint/config'
import cliBaseConfig from '@jalapenolabs/cli/eslint'
import licenseHeader from 'eslint-plugin-license-header'

export default defineConfig([
  {
    extends: [ cliBaseConfig ],
  },
  globalIgnores([
    '.yarn/**',
    '**/vite-env.d.ts',
    // Generated protobuf codegen output. Owned by `yarn generate:proto`; lint
    // rules (license headers, formatting) don't apply to generated artifacts.
    'src/proto/generated/**',
  ]),
  {
    // Override the CLI's default header text to "Jalapeno Labs" (with a space).
    // The plugin is named explicitly: a rule override cannot rely on the shared
    // config's plugin registration surviving that config's internal merging.
    plugins: {
      'license-header': licenseHeader,
    },
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
