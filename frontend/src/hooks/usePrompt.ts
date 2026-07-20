// Copyright © 2026 Jalapeno Labs

// Core
import { useContext } from 'react'

// Misc
import { PromptContext } from '../gates/PromptGate'

// Opens the app-wide prompt modal. Call the returned function with a
// `PromptOptions` (title, optional validator, and an async `onConfirm(value)`).
// See gates/PromptGate.tsx.
export function usePrompt() {
  return useContext(PromptContext)
}
