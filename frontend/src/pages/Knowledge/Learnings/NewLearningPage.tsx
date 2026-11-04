// Copyright © 2026 Jalapeno Labs

// Core
import { useNavigate } from 'react-router'
import { useTranslation } from 'react-i18next'

// User interface
import { Page } from '../../../layout/page/Page'
import { LearningForm } from './LearningForm'

// Misc
import { getLearningUrl, UrlTree } from '../../../urls'

// `/knowledge/learnings/new`: saving a learning by hand. Saving opens the learning, a new one or
// the one whose takeaway it named.
export function NewLearningPage() {
  const { t } = useTranslation([ 'learnings', 'knowledge' ])
  const navigate = useNavigate()

  return <Page
    title={t('new')}
    parents={[
      { label: t('knowledge:title'), href: UrlTree.knowledge },
      { label: t('title'), href: UrlTree.knowledgeLearnings },
    ]}
  >
    <LearningForm
      learning={null}
      onSaved={(learning) => navigate(getLearningUrl(learning.id), { replace: true })}
    />
  </Page>
}
