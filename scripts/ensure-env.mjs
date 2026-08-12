// Copyright © 2026 Jalapeno Labs

// Creates `.env` from `.env.example` when it is missing, so a fresh checkout
// runs `yarn dev` with no setup step. `.env` is gitignored and is yours to
// edit; this script never overwrites one that already exists.

import { copyFileSync, existsSync } from 'node:fs'
import { dirname, join } from 'node:path'
import { fileURLToPath } from 'node:url'

const repositoryRoot = join(dirname(fileURLToPath(import.meta.url)), '..')
const environmentFile = join(repositoryRoot, '.env')

if (existsSync(environmentFile)) {
  process.exit(0)
}

copyFileSync(join(repositoryRoot, '.env.example'), environmentFile)
console.log('Created .env from .env.example. Edit it to change development settings.')
