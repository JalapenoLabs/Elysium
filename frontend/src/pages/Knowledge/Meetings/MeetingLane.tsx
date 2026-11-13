// Copyright © 2026 Jalapeno Labs

import type { ComponentProps } from 'react'
import type { MeetingWord } from '../../../api/routes/meetingRoutes'
import type { ThreadActions } from './CommentThread'
import type { RemovableMarkKind } from './useMeetingChanges'
import type { HistoryRow } from './meetingReviewPresentation'
import type { PlacedMark, WordRange } from './meetingTranscript'
import type { Thread } from './meetingThreads'

// Core
import { useState } from 'react'
import { useTranslation } from 'react-i18next'

// Redux
import { selectMe } from '../../../store/authSlice'
import { useAppSelector } from '../../../store/hooks'

// User interface
import { CommentForm } from './CommentForm'
import { MarkDetail } from './MarkDetail'
import { MeetingFiles } from './MeetingFiles'
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
  // Null where the meeting takes no change, as one merged into another does.
  onRemoveMark: ((markId: string, kind: RemovableMarkKind) => Promise<void>) | null
  // The meeting's files and frames, as the lane lists them and an excerpt's detail pins them.
  files: ComponentProps<typeof MeetingFiles>
  pinning: ComponentProps<typeof MarkDetail>['files']
  threads: {
    open: Thread[]
    resolved: Thread[]
    isLoading: boolean
    hasFailed: boolean
  }
  // Null where the meeting takes no change, as one merged into another does.
  threadActions: ThreadActions | null
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
// files and frames, then its comment threads, which pass under the box as they scroll by, then
// every change to the meeting.
export function MeetingLane(props: Props) {
  const { t } = useTranslation('meetings')
  const [ isShowingAll, setIsShowingAll ] = useState(false)
  const {
    words,
    selectedMark,
    history,
    onUndo,
    onRemoveMark,
    threads,
    threadActions,
    onSelectThread,
    composing,
    isInDrawer,
    files,
    pinning,
    ...options
  } = props
  // An excerpt's record is its meeting's owner's, so only they change its projects; a deleted
  // meeting's records are on no timeline until it is restored, so even they read its projects from
  // its words.
  const ownsRecords = useAppSelector(selectMe)?.user.id === options.meeting.ownerId
    && !options.meeting.deletedAt
  const selectedThread = selectedMark?.kind === 'comment'
    ? [ ...threads.open, ...threads.resolved ].find((thread) => thread.placed.mark.id === selectedMark.mark.id)
      ?? null
    : null

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

    {composing && threadActions && <section
      aria-label={t('review.comments.new')}
      className='rounded-xl border border-border p-4'
    >
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
        mayRead={threadActions.mayRead}
      />
    </section>}

    {!composing && selectedMark && <div className='rounded-xl border border-border p-4'>
      <MarkDetail
        key={selectedMark.mark.id}
        words={words}
        placed={selectedMark}
        history={history.rows}
        onUndo={onUndo}
        onRemove={onRemoveMark}
        thread={selectedThread}
        threadActions={threadActions}
        ownsRecords={ownsRecords}
        files={pinning}
      />
    </div>}

    <MeetingFiles {...files} />

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
