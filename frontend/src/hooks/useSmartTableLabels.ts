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
    moreActions: t('actions.moreActions'),
    searchNoResults: t('table.searchNoResults'),
    done: t('actions.done'),
    rowsPerPage: t('table.rowsPerPage'),
    pagination: t('table.pagination'),
    previousPage: t('table.previousPage'),
    nextPage: t('table.nextPage'),
    exportCsv: t('table.exportCsv'),
    filters: t('table.filters'),
    applyFilters: t('table.applyFilters'),
    cancel: t('actions.cancel'),
    filterAny: t('table.filterAny'),
    filterYes: t('table.filterYes'),
    filterNo: t('table.filterNo'),
    activeFilters: t('table.activeFilters'),
    filterTag: (filterLabel: string, valueText: string) => t('table.filterTag', { filterLabel, valueText }),
    removeFilter: (tagText: string) => t('table.removeFilter', { tagText }),
    activeFilterCount: (count: number) => t('table.activeFilterCount', { count }),
    selectionBar: t('table.selectionBar'),
    selectedCount: (count: number) => t('table.selectedCount', { count }),
    selectPage: t('table.selectPage'),
    clearSelection: t('table.clearSelection'),
    clearSelectionHint: t('table.clearSelectionHint'),
    pageSelected: (count: number) => t('table.pageSelected', { count }),
    selectEverything: (count: number) => t('table.selectEverything', { count }),
    everythingSelected: (count: number) => t('table.everythingSelected', { count }),
    choiceItem: (actionLabel: string, choiceLabel: string) => t('table.choiceItem', { actionLabel, choiceLabel }),
    bulkEdit: t('table.bulkEdit'),
    bulkEditTitle: (count: number) => t('table.bulkEditTitle', { count }),
    bulkEditNothingChosen: t('table.bulkEditNothingChosen'),
    keepAsIs: t('table.keepAsIs'),
    setTo: t('table.setTo'),
    clearField: t('table.clearField'),
    applyBulkEdit: (count: number) => t('table.applyBulkEdit', { count }),
  }), [ t ])
}
