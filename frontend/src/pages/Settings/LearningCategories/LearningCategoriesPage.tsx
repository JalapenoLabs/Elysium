// Copyright © 2026 Jalapeno Labs

import type { LearningCategory } from '../../../api/routes/learningCategoryRoutes'
import type { LearningCategoryRow } from './LearningCategoryTable'

// Core
import { useMemo, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { mutate } from 'swr'

// Redux
import { selectIsAdmin } from '../../../store/authSlice'
import { useAppDispatch, useAppSelector } from '../../../store/hooks'
import {
  learningCategoryRemoved,
  learningCategoryUpserted,
  selectLearningCategories,
} from '../../../store/learningCategoriesSlice'

// User interface
import { buttonVariants, Link, Spinner } from '@heroui/react'
import { LuPlus } from 'react-icons/lu'
import { EmptyNotice } from '../../../components/EmptyNotice'
import { SettingsPage } from '../SettingsPage'
import { LearningCategoryTable } from './LearningCategoryTable'
import { RemoveLearningCategoryModal } from './RemoveLearningCategoryModal'

// Misc
import { getApiErrorMessage } from '../../../api/errors'
import {
  LEARNING_CATEGORIES_KEY,
  removeLearningCategory,
  updateLearningCategory,
} from '../../../api/routes/learningCategoryRoutes'
import { useLearningCategoriesLoader } from '../../../hooks/useServerData'
import { UrlTree } from '../../../urls'
import { presentLearningCategory } from '../Learnings/learningPresentation'
import { toast } from '../../../toast/toast'

// `/settings/learning-categories`: every category learnings are filed under, in the order they
// are offered. Elysium's built-in ones are fixed; admins add, edit, reorder, and remove the
// workspace's own. Anyone else who opens the address is told so, and the settings directory
// hides the entry.
export function LearningCategoriesPage() {
  const { t } = useTranslation([ 'learningCategories', 'common' ])
  const { t: translateLearnings } = useTranslation('learnings')
  const dispatch = useAppDispatch()
  const isAdmin = useAppSelector(selectIsAdmin)
  const status = useLearningCategoriesLoader(isAdmin)
  const stored = useAppSelector(selectLearningCategories)
  const [ removing, setRemoving ] = useState<LearningCategory | null>(null)

  const rows = useMemo<LearningCategoryRow[]>(
    () => stored.map((category) => ({
      stored: category,
      presented: presentLearningCategory(category, translateLearnings),
    })),
    [ stored, translateLearnings ],
  )

  // Moving one shifts the others' places, so the list is loaded again whole.
  async function move(category: LearningCategory, direction: 'up' | 'down') {
    const customKeys: string[] = []
    for (const row of rows) {
      if (!row.stored.builtIn) {
        customKeys.push(row.stored.key)
      }
    }
    const place = customKeys.indexOf(category.key)
    const position = direction === 'up'
      ? place - 1
      : place + 1
    try {
      const response = await updateLearningCategory(category.key, { position })
      dispatch(learningCategoryUpserted(response.category))
      void mutate(LEARNING_CATEGORIES_KEY)
      toast.success(t('toasts.moved'))
    }
    catch (error) {
      console.debug('LearningCategoriesPage could not move a category', { error, key: category.key })
      toast.danger(t('common:errors.unexpected'), { description: getApiErrorMessage(error) ?? undefined })
    }
  }

  async function remove(category: LearningCategory, moveTo: string | null) {
    try {
      await removeLearningCategory(category.key, moveTo)
    }
    catch (error) {
      console.debug('LearningCategoriesPage could not remove a category', { error, key: category.key })
      toast.danger(t('common:errors.unexpected'), { description: getApiErrorMessage(error) ?? undefined })
      // Keeps the dialog open to try again.
      throw error
    }

    dispatch(learningCategoryRemoved(category.key))
    // The category its learnings moved to now holds them.
    void mutate(LEARNING_CATEGORIES_KEY)
    const target = rows.find((row) => row.stored.key === moveTo)
    if (target) {
      toast.success(t('toasts.removedAndMoved', {
        label: category.label,
        target: target.presented.label,
        count: category.learningCount,
      }))
      return
    }
    toast.success(t('toasts.removed', { label: category.label }))
  }

  if (!isAdmin) {
    return <SettingsPage title={t('title')}>
      <EmptyNotice>{t('adminOnly')}</EmptyNotice>
    </SettingsPage>
  }

  return <SettingsPage
    title={t('title')}
    primaryAction={<Link
      href={UrlTree.settingsLearningCategoryNew}
      className={buttonVariants({ variant: 'outline', className: 'gap-2' })}
    >
      <LuPlus className='size-4' aria-hidden />
      <span>{t('add')}</span>
    </Link>}
  >
    {status === 'loading' && <div className='grid place-items-center py-16'>
      <Spinner />
    </div>}

    {status === 'failed' && <p className='py-10 text-center text-sm text-danger'>{
      t('table.loadError')
    }</p>}

    {status === 'loaded' && <LearningCategoryTable
      rows={rows}
      onMove={(category, direction) => void move(category, direction)}
      onRemove={setRemoving}
    />}

    {removing && <RemoveLearningCategoryModal
      key={removing.key}
      category={removing}
      categories={rows.map((row) => row.presented)}
      onRemove={remove}
      onClose={() => setRemoving(null)}
    />}
  </SettingsPage>
}
