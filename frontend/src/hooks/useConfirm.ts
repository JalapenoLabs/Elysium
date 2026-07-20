// Copyright © 2026 Jalapeno Labs

// Core
import { useContext } from 'react'

// Misc
import { ConfirmContext } from '../gates/ConfirmGate'

// Opens the app-wide confirmation modal. Call the returned function with a
// `ConfirmOptions` (title, message, and an async `onConfirm`); the modal handles
// the loading state and dismissal. See gates/ConfirmGate.tsx.
export function useConfirm() {
  return useContext(ConfirmContext)
}
