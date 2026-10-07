// Copyright © 2026 Jalapeno Labs

import type { CodingSession } from '../../api/routes/codingSessionRoutes'
import type { StudioItem } from '../../api/routes/studioRoutes'

// Core
import { useTranslation } from 'react-i18next'

// User interface
import { Card, Chip, Link } from '@heroui/react'
import { RestoreStudioItemButton } from './RestoreStudioItemButton'
import { StudioThumbnail } from './StudioThumbnail'

// Misc
import { getStudioItemUrl } from '../../urls'
import { threadStateChipColors, threadStateLabelKeys } from '../Coding/sessionPresentation'
import { getTileThumbnail } from './studioListing'

// The title's overlay stretches across the whole card, so clicking anywhere on a tile opens
// the item; Restore sits above it, since a button cannot live inside a link. As on the
// session tiles, `static` hands the overlay's positioning up to the card.
const TILE_TITLE_CLASS_NAME = [
  'static block truncate font-medium text-foreground no-underline hover:underline',
  'after:absolute after:inset-0 after:z-[1]',
].join(' ')

type Props = {
  item: StudioItem
  // The item's newest session, whose thread the tile reports; absent until one loads.
  latestSession: CodingSession | undefined
}

// One item in the grid: its picture, title, live thread state, and how many images and
// models it holds.
export function StudioItemTile(props: Props) {
  const { t, i18n } = useTranslation([ 'studio', 'coding' ])
  const item = props.item
  const state = props.latestSession?.thread?.state ?? 'unknown'
  const thumbnail = getTileThumbnail(item, props.latestSession)

  return <Card className='relative h-full overflow-hidden p-0 transition-transform hover:-translate-y-0.5'>
    <StudioThumbnail
      itemId={item.id}
      thumbnail={thumbnail}
      className='aspect-square w-full'
    />
    <div className='flex flex-col gap-2 px-4 pb-4'>
      <Card.Title className='min-w-0'>
        <Link
          href={getStudioItemUrl(item.id)}
          className={TILE_TITLE_CLASS_NAME}
        >{
          item.title
        }</Link>
      </Card.Title>
      <div className='flex flex-wrap items-center gap-2 text-xs'>
        {!item.deletedAt && <Chip size='sm' variant='soft' color={threadStateChipColors[state]}>{
          t(threadStateLabelKeys[state], { ns: 'coding' })
        }</Chip>}
        <span className='opacity-70'>{t('grid.images', { count: item.imageCount })}</span>
        <span className='opacity-70'>{t('grid.models', { count: item.modelCount })}</span>
      </div>
      {item.deletedAt && <div className='level items-center text-xs'>
        <span className='opacity-70'>{
          t('grid.deletedOn', {
            date: new Intl.DateTimeFormat(i18n.language, { dateStyle: 'medium' }).format(new Date(item.deletedAt)),
          })
        }</span>
        <div className='relative z-10'>
          <RestoreStudioItemButton item={item} />
        </div>
      </div>}
    </div>
  </Card>
}
