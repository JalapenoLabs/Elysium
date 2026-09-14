// Copyright © 2026 Jalapeno Labs

// Core
import useSWR from 'swr'

// Misc
import { listLlms } from '../api/routes/llmRoutes'

// Shared cache key so every consumer sees the same list and `mutate` refreshes all.
export const LLMS_CACHE_KEY = 'v1/llms'

export function useLlms() {
  return useSWR(LLMS_CACHE_KEY, listLlms)
}
