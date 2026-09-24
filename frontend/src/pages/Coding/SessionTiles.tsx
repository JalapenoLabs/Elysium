// Copyright © 2026 Jalapeno Labs

import type { SessionSummary } from './sessionPresentation'

// Core
import { useTranslation } from 'react-i18next'

// User interface
import { Card, Chip, Link } from '@heroui/react'
import { SessionRowActions } from './SessionRowActions'

// Misc
import { useCodingActions } from './codingActionsContext'
import { threadStateChipColors } from './sessionPresentation'

// The title's overlay stretches across the whole card, so clicking anywhere on a tile opens
// the session, as the title does in the table. The row menu sits above the overlay, since
// a button cannot live inside another pressable element. HeroUI positions its links, so
// `static` hands the overlay's positioning up to the card, and the overlay's z-index lifts it
// over the faded text, whose opacity would otherwise paint it above.
const TILE_TITLE_CLASS_NAME = [
  'static cursor-pointer font-medium text-link no-underline hover:underline',
  'after:absolute after:inset-0 after:z-[1]',
].join(' ')

type Props = {
  summaries: SessionSummary[]
}

// The tiles view: one card per session with what the table's row shows, in the order the
// sessions arrive (newest first). The grid fits as many columns as the panel is wide.
export function SessionTiles(props: Props) {
  const { t } = useTranslation('coding')
  const actions = useCodingActions()

  return <ul className='grid grid-cols-[repeat(auto-fill,minmax(16rem,1fr))] gap-4'>{
    props.summaries.map((summary) => <li key={summary.session.id}>
      <Card className='relative h-full transition-transform hover:-translate-y-0.5'>
        <Card.Header className='flex-row items-start gap-2'>
          <div className='min-w-0 flex-1'>
            <p className='text-xs tabular-nums opacity-60'>{
              t('sessions.numbered', { number: summary.session.id })
            }</p>
            <Card.Title className='truncate'>
              <Link
                className={TILE_TITLE_CLASS_NAME}
                onPress={() => actions.openSession(summary.session)}
              >{
                summary.session.title
              }</Link>
            </Card.Title>
          </div>
          <div className='relative z-10 shrink-0'>
            <SessionRowActions session={summary.session} />
          </div>
        </Card.Header>
        <Card.Content>
          <Chip
            className='compact'
            size='sm'
            variant='soft'
            color={threadStateChipColors[summary.state]}
          >{
            summary.stateLabel
          }</Chip>
          <dl className='grid grid-cols-[auto_minmax(0,1fr)] gap-x-3 gap-y-1 text-sm'>
            <dt className='opacity-60'>{t('sessions.project')}</dt>
            <dd className='truncate'>{summary.projectName}</dd>
            <dt className='opacity-60'>{t('sessions.satellite')}</dt>
            <dd className='truncate'>{summary.satelliteName}</dd>
          </dl>
        </Card.Content>
        <Card.Footer className='gap-2 text-xs opacity-70'>
          <span>{t('sessions.lastActivity')}</span>
          <span className='ml-auto'>{summary.lastActivity}</span>
        </Card.Footer>
      </Card>
    </li>)
  }</ul>
}
