// Copyright © 2026 Jalapeno Labs

// Core
import { useNavigate, useParams } from 'react-router'
import { useTranslation } from 'react-i18next'

// Redux
import { selectIsAdmin } from '../../../store/authSlice'
import { useAppSelector } from '../../../store/hooks'
import { selectLearningCategoryByKey } from '../../../store/learningCategoriesSlice'

// User interface
import { Link, Spinner } from '@heroui/react'
import { EmptyNotice } from '../../../components/EmptyNotice'
import { SettingsPage } from '../SettingsPage'
import { LearningCategoryForm } from './LearningCategoryForm'

// Misc
import { useLearningCategoriesLoader } from '../../../hooks/useServerData'
import { UrlTree } from '../../../urls'

// `/settings/learning-categories/:categoryKey/edit`: renaming or describing one of the
// workspace's own categories. Admins only; a built-in category is named, not edited.
export function EditLearningCategoryPage() {
  const { t } = useTranslation('learningCategories')
  const navigate = useNavigate()
  const { categoryKey = '' } = useParams()
  const isAdmin = useAppSelector(selectIsAdmin)
  const status = useLearningCategoriesLoader(isAdmin)
  const category = useAppSelector((state) => selectLearningCategoryByKey(state, categoryKey))
  const parents = [{ label: t('title'), href: UrlTree.settingsLearningCategories }]

  if (!isAdmin) {
    return <SettingsPage title={t('title')} parents={parents}>
      <EmptyNotice>{t('adminOnly')}</EmptyNotice>
    </SettingsPage>
  }

  if (!category) {
    if (status === 'loading') {
      return <div className='grid place-items-center py-16'>
        <Spinner />
      </div>
    }
    return <div className='container py-10 text-center text-sm'>
      <p className='compact opacity-70'>{t('page.notFound')}</p>
      <Link href={UrlTree.settingsLearningCategories} className='text-link'>{t('page.backToList')}</Link>
    </div>
  }

  if (category.builtIn) {
    return <SettingsPage title={category.label} parents={parents}>
      <EmptyNotice>{t('page.builtIn', { label: category.label })}</EmptyNotice>
    </SettingsPage>
  }

  return <SettingsPage
    title={t('editTitle', { label: category.label })}
    parents={parents}
  >
    <LearningCategoryForm
      // Remount when navigating between categories so the form takes the new values.
      key={category.key}
      category={category}
      onSaved={() => navigate(UrlTree.settingsLearningCategories)}
    />
  </SettingsPage>
}
