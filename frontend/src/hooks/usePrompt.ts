// Copyright © 2026 Jalapeno Labs

// Core
import { useContext } from 'react'

// Misc
import { PromptContext } from '../gates/PromptGate'

// Opens the app's single-field dialog. See `PromptOptions` in src/gates/PromptGate.tsx.
export function usePrompt() {
  return useContext(PromptContext)
}
