// Copyright © 2026 Jalapeno Labs

import type { ComponentProps } from 'react'
import type { MeetingWord } from '../../../api/routes/meetingRoutes'
import type { HistoryRow } from './meetingReviewPresentation'
import type { PlacedMark } from './meetingTranscript'

// Core
import { useState } from 'react'
import { useTranslation } from 'react-i18next'

// User interface
import { MarkDetail } from './MarkDetail'
import { MeetingHistory } from './MeetingHistory'
import { MeetingOptions } from './MeetingOptions'

type Props = ComponentProps<typeof MeetingOptions> & {
  words: MeetingWord[]
  selectedMark: PlacedMark | null
  history: {
    rows: HistoryRow[]
    isLoading: boolean
    hasFailed: boolean
  }
  onUndo: (entryId: string) => Promise<void>
  // In the phone's drawer the lane scrolls on its own, so nothing needs to float.
  isInDrawer?: boolean
}

// The right lane of a meeting's page (docs/meeting-review.md, The meeting's page): the options box
// floating at its top, then the mark selected on the transcript, then every change to the meeting.
// Comments (#245) take the place below the options as they are built, passing under the box as
// they scroll by.
export function MeetingLane(props: Props) {
  const { t } = useTranslation('meetings')
  const [ isShowingAll, setIsShowingAll ] = useState(false)
  const { words, selectedMark, history, onUndo, isInDrawer, ...options } = props

  // The drawer quiets its text; the lane reads as the page does wherever it sits. Beside the
  // transcript the lane fills its grid cell, the transcript's full height, so the options box has
  // the whole page to stay stuck in; sized to its own content, it would scroll away with it. Stuck,
  // it is never taller than the page's scroller below the topbar, as the global search's results
  // are not, and scrolls on its own past that, or its foot (the captures, the meeting's actions)
  // would stay out of reach until the transcript ended.
  return <div className={[ 'flex flex-col gap-4 text-foreground', isInDrawer ? '' : 'h-full' ].join(' ')}>
    <div className={[
      'rounded-xl border border-border bg-surface p-4',
      isInDrawer
        ? ''
        : 'sticky top-0 z-10 max-h-[calc(100dvh-5rem)] overflow-y-auto shadow-sm',
    ].join(' ')}>
      <MeetingOptions {...options} />
    </div>

    {selectedMark && <div className='rounded-xl border border-border p-4'>
      <MarkDetail
        key={selectedMark.mark.id}
        words={words}
        placed={selectedMark}
        history={history.rows}
        onUndo={onUndo}
      />
    </div>}

    <section aria-label={t('review.history.title')} className='px-1'>
      <p className='compact text-xs font-semibold uppercase opacity-60'>{t('review.history.title')}</p>
      <MeetingHistory
        rows={history.rows}
        isLoading={history.isLoading}
        hasFailed={history.hasFailed}
        isShowingAll={isShowingAll}
        onShowAll={() => setIsShowingAll(true)}
        onUndo={onUndo}
      />
    </section>
  </div>
}
