// Copyright © 2026 Jalapeno Labs

import { defineConfig, globalIgnores } from 'eslint/config'
import cliBaseConfig from '@jalapenolabs/cli/eslint'

export default defineConfig([
  {
    extends: [ cliBaseConfig ],
  },
  globalIgnores([
    '.yarn/**',
    '**/vite-env.d.ts',
    // Ambient declaration that augments the global `Window` (needs `interface`,
    // which the shared config otherwise discourages), like vite-env.d.ts.
    '**/electron.d.ts',
    // Generated protobuf codegen output. Owned by `yarn generate:proto`; lint
    // rules (license headers, formatting) don't apply to generated artifacts.
    'src/proto/generated/**',
  ]),
  {
    // Override the CLI's default header text to "Jalapeno Labs" (with a space).
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
