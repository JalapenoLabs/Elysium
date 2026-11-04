// Copyright © 2026 Jalapeno Labs

// Core
import { useNavigate } from 'react-router'
import { useTranslation } from 'react-i18next'

// Redux
import { selectIsAdmin } from '../../../store/authSlice'
import { useAppSelector } from '../../../store/hooks'

// User interface
import { EmptyNotice } from '../../../components/EmptyNotice'
import { SettingsPage } from '../SettingsPage'
import { LearningCategoryForm } from './LearningCategoryForm'

// Misc
import { UrlTree } from '../../../urls'

// `/settings/learning-categories/new`: adding one of the workspace's own categories, after the
// last of them. Admins only.
export function NewLearningCategoryPage() {
  const { t } = useTranslation('learningCategories')
  const navigate = useNavigate()
  const isAdmin = useAppSelector(selectIsAdmin)

  return <SettingsPage
    title={t('newTitle')}
    parents={[{ label: t('title'), href: UrlTree.settingsLearningCategories }]}
  >
    {isAdmin
      ? <LearningCategoryForm
        category={null}
        onSaved={() => navigate(UrlTree.settingsLearningCategories)}
      />
      : <EmptyNotice>{t('adminOnly')}</EmptyNotice>}
  </SettingsPage>
}
