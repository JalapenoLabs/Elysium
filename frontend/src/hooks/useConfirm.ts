// Copyright © 2026 Jalapeno Labs

// Core
import { useContext } from 'react'

// Misc
import { ConfirmContext } from '../gates/ConfirmGate'

// Opens the app's confirmation dialog. See `ConfirmOptions` in src/gates/ConfirmGate.tsx.
export function useConfirm() {
  return useContext(ConfirmContext)
}
