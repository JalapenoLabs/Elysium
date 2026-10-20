// Copyright © 2026 Jalapeno Labs

// Core
import { useTranslation } from 'react-i18next'

// Redux
import { useAppDispatch, useAppSelector } from '../store/hooks'
import { elysiaTurnUpserted, selectElysiaTurns } from '../store/elysiaSlice'

// User interface
import { Button, Chip, Spinner, toast } from '@heroui/react'

// Misc
import { cancelElysiaTurn } from '../api/routes/elysiaRoutes'
import { useElysiaTurnsLoader } from '../hooks/useServerData'
import { triggerLabelKey, turnStatusLabelKeys } from './presentation'

// Elysia's turns, newest first: what started each, how it ended, and her one or two
// sentence summary of what it did.
export function ElysiaActivity() {
  const { t, i18n } = useTranslation('elysia')
  const dispatch = useAppDispatch()
  const status = useElysiaTurnsLoader()
  const turns = useAppSelector(selectElysiaTurns)
  const timeFormatter = new Intl.DateTimeFormat(i18n.language, { dateStyle: 'short', timeStyle: 'short' })

  async function cancel(turnId: string) {
    try {
      const response = await cancelElysiaTurn(turnId)
      dispatch(elysiaTurnUpserted(response.turn))
    }
    catch (error) {
      console.debug('ElysiaActivity failed to cancel a turn', { error, turnId })
      toast.danger(t('activity.cancelFailed'))
    }
  }

  if (status === 'loading') {
    return <div className='grid place-items-center py-6'>
      <Spinner size='sm' />
    </div>
  }
  if (status === 'failed') {
    return <p className='text-sm text-danger'>{t('activity.loadFailed')}</p>
  }
  if (!turns.length) {
    return <p className='text-sm opacity-70'>{t('activity.empty')}</p>
  }

  return <ol className='flex flex-col gap-2 overflow-y-auto'>{
    turns.map((turn) => <li key={turn.id} className='rounded-xl bg-surface p-3'>
      <div className='level gap-2 text-xs'>
        <span>
          <span className='font-semibold'>{t(triggerLabelKey(turn.triggerKind))}</span>
          <span className='ml-2 opacity-60'>{timeFormatter.format(new Date(turn.createdAt))}</span>
        </span>
        <Chip size='sm' variant='soft' color={turn.status === 'failed'
          ? 'danger'
          : 'default'}>{
            t(turnStatusLabelKeys[turn.status])
          }</Chip>
      </div>
      <p className='mt-1 text-sm whitespace-pre-line'>{
        turn.error ?? turn.summary ?? t('activity.noSummary')
      }</p>
      {turn.status === 'queued' && <div className='mt-2 flex justify-end'>
        <Button size='sm' variant='ghost' onPress={() => void cancel(turn.id)}>
          <span>{t('activity.cancel')}</span>
        </Button>
      </div>}
    </li>)
  }</ol>
}
