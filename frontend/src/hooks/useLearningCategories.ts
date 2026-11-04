// Copyright © 2026 Jalapeno Labs

// Core
import { useMemo } from 'react'
import { useTranslation } from 'react-i18next'

// Redux
import { selectIsCompanyMember } from '../store/authSlice'
import { useAppSelector } from '../store/hooks'
import { selectLearningCategories } from '../store/learningCategoriesSlice'

// Misc
import { presentLearningCategory } from '../pages/Knowledge/Learnings/learningPresentation'
import { useLearningCategoriesLoader } from './useServerData'

// Every learning category as people read it, built-in ones first, loaded for the company's
// members, with each one's label by key. Callers show a key the list does not have, such as
// one removed since, as the key itself.
export function useLearningCategories() {
  const { t } = useTranslation('learnings')
  const isCompanyMember = useAppSelector(selectIsCompanyMember)
  const status = useLearningCategoriesLoader(isCompanyMember)
  const stored = useAppSelector(selectLearningCategories)

  const presented = useMemo(() => {
    const categories = stored.map((category) => presentLearningCategory(category, t))
    const labels: Record<string, string> = {}
    for (const category of categories) {
      labels[category.key] = category.label
    }
    return { categories, labels } as const
  }, [ stored, t ])

  return {
    status,
    categories: presented.categories,
    labels: presented.labels,
  } as const
}
