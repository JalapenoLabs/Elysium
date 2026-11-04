// Copyright © 2026 Jalapeno Labs

import type { CompanyDocHistoryEntry } from '../../../api/routes/companyDocRoutes'

// Core
import { useState } from 'react'
import { useTranslation } from 'react-i18next'
import useSWR from 'swr'

// Redux
import { companyDocLoaded } from '../../../store/companyDocSlice'
import { useAppDispatch } from '../../../store/hooks'

// User interface
import { Button, Spinner, Tooltip } from '@heroui/react'
import { LuUndo2 } from 'react-icons/lu'
import { DateText } from '../../../format/DateText'

// Misc
import { getApiErrorMessage } from '../../../api/errors'
import {
  COMPANY_DOC_HISTORY_KEY,
  listCompanyDocHistory,
  undoCompanyDocChange,
} from '../../../api/routes/companyDocRoutes'
import { useUserNames } from '../../../hooks/useUserNames'
import { describeHistoryEntry, historySentenceKeys, sectionTitleKeys } from './companyDocPresentation'
import { toast } from '../../../toast/toast'

// Every change to the doc, newest first: who made it, which sections it changed, and when, each
// with Undo. Read from SWR alone, since no other view shows it; `companyDoc.changed` revalidates
// it, so a change made anywhere appears here.
export function CompanyDocHistory() {
  const { t, i18n } = useTranslation([ 'companyDoc', 'common' ])
  const dispatch = useAppDispatch()
  const userNames = useUserNames()
  const history = useSWR(COMPANY_DOC_HISTORY_KEY, listCompanyDocHistory)
  const [ undoingId, setUndoingId ] = useState<string | null>(null)

  async function undo(entry: CompanyDocHistoryEntry) {
    setUndoingId(entry.id)
    try {
      const response = await undoCompanyDocChange(entry.id)
      dispatch(companyDocLoaded(response))
      void history.mutate()
      toast.success(t('toasts.undone'))
    }
    catch (error) {
      // A change with nothing left to reverse answers 409 with the reason, in the API's words.
      console.debug('CompanyDocHistory could not undo a change', { error, entryId: entry.id })
      toast.danger(t('common:errors.unexpected'), { description: getApiErrorMessage(error) ?? undefined })
    }
    finally {
      setUndoingId(null)
    }
  }

  if (history.error) {
    return <p className='text-sm text-danger'>{t('history.loadError')}</p>
  }
  if (!history.data) {
    return <div className='grid place-items-center py-6'>
      <Spinner size='sm' />
    </div>
  }
  if (!history.data.history.length) {
    return <p className='text-sm opacity-70'>{t('history.empty')}</p>
  }

  const listFormatter = new Intl.ListFormat(i18n.language, { type: 'conjunction' })

  return <ol className='flex flex-col gap-4 border-l border-separator pl-4'>{
    history.data.history.toReversed().map((entry) => {
      const description = describeHistoryEntry(entry)
      const who = userNames[entry.createdBy] ?? t('common:table.someone')
      const sections = listFormatter.format(description.sections.map((section) => t(sectionTitleKeys[section])))
      const sentence = description.kind
        ? t(historySentenceKeys[description.kind], { who, sections })
        : t('history.entries.unknown', { who })

      return <li key={entry.id} className='level items-start gap-4 text-sm'>
        <div className='min-w-0'>
          <p>{sentence}</p>
          {description.undid && <p className='mt-1 text-xs opacity-70'>{t('history.undid')}</p>}
          <div className='mt-1 text-xs opacity-50'>
            <DateText value={entry.createdAt} />
          </div>
        </div>
        <Tooltip delay={300}>
          <Button
            isIconOnly
            size='sm'
            variant='ghost'
            aria-label={t('history.undo')}
            isPending={undoingId === entry.id}
            isDisabled={undoingId !== null}
            onPress={() => undo(entry)}
          >
            <LuUndo2 className='size-4' aria-hidden />
          </Button>
          <Tooltip.Content>
            <span>{t('history.undo')}</span>
          </Tooltip.Content>
        </Tooltip>
      </li>
    })
  }</ol>
}
