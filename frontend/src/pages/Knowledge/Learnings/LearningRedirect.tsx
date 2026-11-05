// Copyright © 2026 Jalapeno Labs

// Core
import { Navigate, useParams } from 'react-router'

// Misc
import { getKnowledgePageUrl } from '../../../urls'

// `/knowledge/learnings/:learningId`, the address links and bookmarks to a learning hold. A learning
// is a page with the same id, so it lands there.
export function LearningRedirect() {
  const { learningId = '' } = useParams()

  return <Navigate to={getKnowledgePageUrl(learningId)} replace />
}
