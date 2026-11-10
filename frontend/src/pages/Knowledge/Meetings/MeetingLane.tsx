// Copyright © 2026 Jalapeno Labs

import type { ComponentProps } from 'react'
import type { MeetingWord } from '../../../api/routes/meetingRoutes'
import type { ThreadActions } from './CommentThread'
import type { HistoryRow } from './meetingReviewPresentation'
import type { PlacedMark, WordRange } from './meetingTranscript'
import type { Thread } from './meetingThreads'

// Core
import { useState } from 'react'
import { useTranslation } from 'react-i18next'

// User interface
import { CommentForm } from './CommentForm'
import { MarkDetail } from './MarkDetail'
import { MeetingHistory } from './MeetingHistory'
import { MeetingOptions } from './MeetingOptions'
import { MeetingThreads } from './MeetingThreads'

// Misc
import { textOfRange } from './meetingTranscript'

type Props = ComponentProps<typeof MeetingOptions> & {
  words: MeetingWord[]
  selectedMark: PlacedMark | null
  history: {
    rows: HistoryRow[]
    isLoading: boolean
    hasFailed: boolean
  }
  onUndo: (entryId: string) => Promise<void>
  threads: {
    open: Thread[]
    resolved: Thread[]
    isLoading: boolean
    hasFailed: boolean
  }
  threadActions: ThreadActions
  onSelectThread: (threadId: string) => void
  // The words a new thread is being started on, while one is.
  composing: {
    range: WordRange
    onPost: (body: string, mentions: string[]) => Promise<boolean>
    onCancel: () => void
  } | null
  // In the phone's drawer the lane scrolls on its own, so nothing needs to float.
  isInDrawer?: boolean
}

// The right lane of a meeting's page (docs/meeting-review.md, The meeting's page): the options box
// floating at its top, then a new thread being written or the mark selected, then the meeting's
// comment threads, which pass under the box as they scroll by, then every change to the meeting.
export function MeetingLane(props: Props) {
  const { t } = useTranslation('meetings')
  const [ isShowingAll, setIsShowingAll ] = useState(false)
  const {
    words,
    selectedMark,
    history,
    onUndo,
    threads,
    threadActions,
    onSelectThread,
    composing,
    isInDrawer,
    ...options
  } = props
  const selectedThread = selectedMark?.kind === 'comment'
    ? [ ...threads.open, ...threads.resolved ].find((thread) => thread.placed.mark.id === selectedMark.mark.id)
      ?? null
    : null

  // The drawer quiets its text; the lane reads as the page does wherever it sits. Beside the
  // transcript the lane is as tall as it, so the options box floats the whole way down.
  return <div className={[
    'flex flex-col gap-4 text-foreground',
    isInDrawer
      ? ''
      : 'h-full',
  ].join(' ')}>
    <div className={[
      'rounded-xl border border-border bg-surface p-4',
      isInDrawer
        ? ''
        : 'sticky top-0 z-10 shadow-sm',
    ].join(' ')}>
      <MeetingOptions {...options} />
    </div>

    {composing && <section aria-label={t('review.comments.new')} className='rounded-xl border border-border p-4'>
      <p className='compact text-sm font-semibold'>{t('review.comments.new')}</p>
      <blockquote className='compact line-clamp-3 border-l-2 border-separator pl-2 text-sm opacity-90'>{
        textOfRange(words, composing.range)
      }</blockquote>
      <CommentForm
        draftName={`meeting-thread-new.${options.meeting.id}`}
        label={t('review.comments.label')}
        submitLabel={t('review.comments.post')}
        onSubmit={composing.onPost}
        onCancel={composing.onCancel}
        focusOnOpen
      />
    </section>}

    {!composing && selectedMark && <div className='rounded-xl border border-border p-4'>
      <MarkDetail
        key={selectedMark.mark.id}
        words={words}
        placed={selectedMark}
        history={history.rows}
        onUndo={onUndo}
        thread={selectedThread}
        threadActions={threadActions}
      />
    </div>}

    <MeetingThreads
      words={words}
      open={threads.open}
      resolved={threads.resolved}
      selectedThreadId={selectedThread?.placed.mark.id ?? null}
      onSelectThread={onSelectThread}
      isLoading={threads.isLoading}
      hasFailed={threads.hasFailed}
    />

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
