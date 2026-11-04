// Copyright © 2026 Jalapeno Labs

import type { LearningHistoryEntry } from '../../../api/routes/learningRoutes'

// Core
import { useState } from 'react'
import { useTranslation } from 'react-i18next'
import { shallowEqual } from 'react-redux'
import useSWR from 'swr'

// Redux
import { useAppDispatch, useAppSelector } from '../../../store/hooks'
import { learningUpserted } from '../../../store/learningsSlice'
import { selectUserNamesById } from '../../../store/usersSlice'

// User interface
import { Button, Spinner, toast, Tooltip } from '@heroui/react'
import { LuUndo2 } from 'react-icons/lu'

// Misc
import { getApiErrorMessage } from '../../../api/errors'
import { learningHistoryKey, listLearningHistory, undoLearningChange } from '../../../api/routes/learningRoutes'
import { useUsersLoader } from '../../../hooks/useServerData'
import { describeHistoryEntry, fieldLabelKeys, historySentenceKeys } from './learningPresentation'

type Props = {
  learningId: string
}

// A learning's history, newest first: who changed it, through which agent, what they changed,
// and when, each change with Undo. It is read from SWR alone, since no other view shows it, and
// `learning.changed` revalidates it, so an undo made anywhere appears here.
export function LearningHistory(props: Props) {
  const { t, i18n } = useTranslation([ 'learnings', 'common' ])
  const dispatch = useAppDispatch()
  useUsersLoader()
  const userNames = useAppSelector(selectUserNamesById, shallowEqual)
  const history = useSWR(learningHistoryKey(props.learningId), () => listLearningHistory(props.learningId))
  const [ undoingId, setUndoingId ] = useState<string | null>(null)

  async function undo(entry: LearningHistoryEntry) {
    setUndoingId(entry.id)
    try {
      const response = await undoLearningChange(props.learningId, entry.id)
      dispatch(learningUpserted(response.learning))
      void history.mutate()
      toast.success(t('toasts.undone'))
    }
    catch (error) {
      // A change with nothing left to reverse answers 409 with the reason, in the API's words.
      console.debug('LearningHistory could not undo a change', { error, entryId: entry.id })
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

  const dateFormatter = new Intl.DateTimeFormat(i18n.language, { dateStyle: 'medium', timeStyle: 'short' })
  const listFormatter = new Intl.ListFormat(i18n.language, { type: 'conjunction' })

  return <ol className='flex flex-col gap-4 border-l border-separator pl-4'>{
    history.data.history.toReversed().map((entry) => {
      const description = describeHistoryEntry(entry)
      const name = userNames[entry.createdBy] ?? t('common:table.someone')
      const who = entry.agent
        ? t('history.byAgent', { name, agent: entry.agent.name ?? t('agent.unnamed') })
        : name
      const fields = listFormatter.format(description.changedFields.map((field) => t(fieldLabelKeys[field])))
      const sentence = description.kind
        ? t(historySentenceKeys[description.kind], { who, fields })
        : t('history.entries.unknown', { who })

      return <li key={entry.id} className='level items-start gap-4 text-sm'>
        <div className='min-w-0'>
          <p>{sentence}</p>
          {description.undid && <p className='mt-1 text-xs opacity-70'>{t('history.undid')}</p>}
          <p className='mt-1 text-xs opacity-50'>{dateFormatter.format(new Date(entry.createdAt))}</p>
        </div>
        <Tooltip delay={300}>
          <Tooltip.Trigger>
            <Button
              isIconOnly
              size='sm'
              variant='ghost'
              aria-label={t('history.undoLabel')}
              isPending={undoingId === entry.id}
              isDisabled={undoingId !== null}
              onPress={() => undo(entry)}
            >
              <LuUndo2 className='size-4' aria-hidden />
            </Button>
          </Tooltip.Trigger>
          <Tooltip.Content>
            <span>{t('history.undoLabel')}</span>
          </Tooltip.Content>
        </Tooltip>
      </li>
    })
  }</ol>
}
