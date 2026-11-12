// Copyright © 2026 Jalapeno Labs

import type { Selection } from 'react-aria-components'
import type { Meeting, MeetingCapture } from '../../../api/routes/meetingRoutes'
import type { DrawnMarkKind, RangeSummary } from './meetingTranscript'

// Core
import { useTranslation } from 'react-i18next'

// User interface
import { Label, Switch, ToggleButton, ToggleButtonGroup } from '@heroui/react'
import { MeetingActions } from './MeetingActions'
import { MeetingVisibility } from './MeetingVisibility'

// Misc
import { formatDuration, formatMediaPosition } from '../../../format/durations'
import { formatNumber } from '../../../format/numbers'
import { meetingSourceNameKey } from '../../Settings/Meetings/meetingsPresentation'
import { markKindLabelKeys } from './meetingReviewPresentation'
import { DRAWN_MARK_KINDS } from './meetingTranscript'

// What the transcript shows: the final one, or one capture's own by its id.
export const FINAL_VIEW = 'final'

type Props = {
  meeting: Meeting
  captures: MeetingCapture[]
  view: string
  onViewChange: (view: string) => void
  shown: ReadonlySet<DrawnMarkKind>
  onShownChange: (kind: DrawnMarkKind, isShown: boolean) => void
  // The words selected, or the moment a click picked, on the meeting's clock.
  span: RangeSummary | null
  momentMs: number | null
}

// The options box at the top of a meeting's lane (docs/meeting-review.md, The meeting's page):
// which transcript shows, which marks, what is selected, and the meeting's actions. It floats
// above the rest of the lane as the page scrolls.
export function MeetingOptions(props: Props) {
  const { t } = useTranslation([ 'meetings', 'connectors' ])
  const isFinal = props.view === FINAL_VIEW

  function changeView(keys: Selection) {
    // The group disallows an empty selection, so one view is always chosen.
    const [ view ] = keys === 'all'
      ? []
      : [ ...keys ]
    if (typeof view === 'string') {
      props.onViewChange(view)
    }
  }

  let selected = t('review.options.nothingSelected')
  if (props.span) {
    selected = t('review.options.span', {
      start: formatMediaPosition(props.span.startMs),
      end: formatMediaPosition(props.span.endMs),
      length: formatDuration(props.span.endMs - props.span.startMs),
      count: props.span.wordCount,
      words: formatNumber(props.span.wordCount),
    })
  }
  else if (props.momentMs !== null) {
    selected = t('review.options.moment', { at: formatMediaPosition(props.momentMs) })
  }

  return <section aria-label={t('review.options.label')} className='flex flex-col gap-4'>
    <div>
      <p className='compact text-xs font-semibold uppercase opacity-60'>{t('review.options.transcript')}</p>
      <ToggleButtonGroup
        aria-label={t('review.options.transcript')}
        selectionMode='single'
        disallowEmptySelection
        size='sm'
        className='flex-wrap'
        selectedKeys={[ props.view ]}
        onSelectionChange={changeView}
      >
        <ToggleButton id={FINAL_VIEW}>
          <span>{t('review.options.final')}</span>
        </ToggleButton>
        {props.captures.map((capture) => <ToggleButton key={capture.id} id={capture.id}>
          <ToggleButtonGroup.Separator />
          <span>{t(meetingSourceNameKey(capture.sourceKind))}</span>
        </ToggleButton>)}
      </ToggleButtonGroup>
    </div>

    {isFinal && <div>
      <p className='compact text-xs font-semibold uppercase opacity-60'>{t('review.options.marks')}</p>
      <div className='flex flex-col gap-2'>{
        DRAWN_MARK_KINDS.map((kind) => <Switch
          key={kind}
          size='sm'
          isSelected={props.shown.has(kind)}
          onChange={(isShown) => props.onShownChange(kind, isShown)}
        >
          <Switch.Content>
            <Switch.Control>
              <Switch.Thumb />
            </Switch.Control>
            <Label className='flex items-center gap-2 text-sm'>
              <span aria-hidden className={`size-2.5 rounded-full meeting-mark-swatch--${kind}`} />
              <span>{t(markKindLabelKeys[kind])}</span>
            </Label>
          </Switch.Content>
        </Switch>)
      }</div>
    </div>}

    {isFinal && <div>
      <p className='compact text-xs font-semibold uppercase opacity-60'>{t('review.options.selected')}</p>
      <p className='text-sm'>{selected}</p>
    </div>}

    <MeetingVisibility meeting={props.meeting} />

    <MeetingActions meeting={props.meeting} />
  </section>
}
