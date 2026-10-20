// Copyright © 2026 Jalapeno Labs

import type { CodingSession, SessionEvent } from '../../api/routes/codingSessionRoutes'
import type { StudioFeedback } from '../../api/routes/studioRoutes'

// Core
import { useTranslation } from 'react-i18next'

// Redux
import { useAppSelector } from '../../store/hooks'
import { selectSessionTimeline } from '../../store/sessionEventsSlice'

// User interface
import { Button, Spinner } from '@heroui/react'
import { ImagePreview } from '../../components/ImagePreview'
import { TimelineEventList } from '../Coding/TimelineEventList'

// Misc
import { getStudioFeedbackImageUrl } from '../../api/routes/studioRoutes'
import { useSessionHistoryLoader } from '../../hooks/useServerData'

type Props = {
  itemId: string
  session: CodingSession
  // The item's drawn prompts by the turn they started, to show each drawing under its prompt.
  feedbackByTurnId: Map<string, StudioFeedback>
}

// One of an item's sessions inside the item's single conversation column. Its events stay
// in Redux while it is shown, as a Coding conversation panel's do.
export function StudioSessionTimeline(props: Props) {
  const { t } = useTranslation([ 'studio', 'common' ])
  const history = useSessionHistoryLoader(props.session.id)
  const timeline = useAppSelector((state) => selectSessionTimeline(state, props.session.id))

  if (!timeline?.isHistoryLoaded) {
    if (history.error) {
      return <div className='py-4 text-center'>
        <p className='compact text-sm text-danger'>{t('conversation.loadError')}</p>
        <Button size='sm' variant='outline' onPress={history.retry}>
          <span>{t('common:actions.retry')}</span>
        </Button>
      </div>
    }
    return <div className='flex items-center justify-center gap-2 py-4 text-sm opacity-70'>
      <Spinner size='sm' />
      <span>{t('conversation.loading')}</span>
    </div>
  }

  function renderDrawing(event: SessionEvent) {
    if (event.payload?.kind !== 'turnStarted' || !event.turnId) {
      return null
    }
    const feedback = props.feedbackByTurnId.get(event.turnId)
    if (!feedback) {
      return null
    }

    const src = getStudioFeedbackImageUrl(props.itemId, feedback.id, 'annotated')
    return <div className='mt-2 flex justify-end'>
      <ImagePreview src={src} alt={t('conversation.drawing')} className='block rounded-lg'>
        <img
          src={src}
          alt={t('conversation.drawing')}
          loading='lazy'
          className='max-h-40 max-w-60 rounded-lg border border-separator object-contain'
        />
      </ImagePreview>
    </div>
  }

  return <>
    {timeline.truncated && <p className='relaxed text-center text-xs opacity-50'>{
      t('conversation.truncated', { ns: 'coding' })
    }</p>}
    <TimelineEventList
      events={timeline.events}
      renderAfterEvent={renderDrawing}
    />
  </>
}
