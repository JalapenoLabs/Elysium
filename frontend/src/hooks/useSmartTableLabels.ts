// Copyright © 2026 Jalapeno Labs

import type { SmartTableLabels } from '@jalapenolabs/uikit'

// Core
import { useMemo } from 'react'
import { useTranslation } from 'react-i18next'

// SmartTable ships English strings and stays i18n-agnostic; this feeds it ours.
export function useSmartTableLabels(): SmartTableLabels {
  const { t } = useTranslation('common')

  return useMemo(() => ({
    searchPlaceholder: t('table.searchPlaceholder'),
    clearSearch: t('table.clearSearch'),
    results: (count: number) => t('table.results', { count }),
    columns: t('table.columns'),
    sort: t('table.sort'),
    sortAscending: t('table.sortAscending'),
    sortDescending: t('table.sortDescending'),
    move: t('table.move'),
    select: t('table.select'),
    selectAll: t('table.selectAll'),
    actions: t('table.actions'),
    searchNoResults: t('table.searchNoResults'),
    rowsPerPage: t('table.rowsPerPage'),
    pagination: t('table.pagination'),
    previousPage: t('table.previousPage'),
    nextPage: t('table.nextPage'),
  }), [ t ])
}
