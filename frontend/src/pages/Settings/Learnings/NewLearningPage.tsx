// Copyright © 2026 Jalapeno Labs

// Core
import { useNavigate } from 'react-router'
import { useTranslation } from 'react-i18next'

// User interface
import { SettingsPage } from '../SettingsPage'
import { LearningForm } from './LearningForm'

// Misc
import { getLearningUrl, UrlTree } from '../../../urls'

// `/settings/learnings/new`: saving a learning by hand. Saving opens the learning, a new one or
// the one whose takeaway it named.
export function NewLearningPage() {
  const { t } = useTranslation('learnings')
  const navigate = useNavigate()

  return <SettingsPage
    title={t('new')}
    parents={[{ label: t('title'), href: UrlTree.settingsLearnings }]}
  >
    <LearningForm
      learning={null}
      onSaved={(learning) => navigate(getLearningUrl(learning.id), { replace: true })}
    />
  </SettingsPage>
}
