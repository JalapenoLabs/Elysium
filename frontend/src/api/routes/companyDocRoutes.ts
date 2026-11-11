// Copyright © 2026 Jalapeno Labs

// Misc
import { apiClient } from '../index'

// The company doc's sections are pages, read, edited, and reviewed through `pageRoutes.ts`
// (docs/knowledge.md, The company doc). What is the doc's own is starting its review.

type ReviewResponse = {
  run: { id: string, state: string }
  // False when a review was already under way, which is answered instead.
  started: boolean
}

export function startCompanyDocReview() {
  return apiClient
    .post('v1/company-doc/review')
    .json<ReviewResponse>()
}
