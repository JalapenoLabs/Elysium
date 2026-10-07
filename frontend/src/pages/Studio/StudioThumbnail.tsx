// Copyright © 2026 Jalapeno Labs

import type { TileThumbnail } from './studioListing'

// Core
import { useTranslation } from 'react-i18next'

// User interface
import { Spinner } from '@heroui/react'
import { LuImageOff } from 'react-icons/lu'

// Misc
import { getStudioAssetContentUrl } from '../../api/routes/studioRoutes'

type Props = {
  itemId: string
  thumbnail: TileThumbnail
  className?: string
}

// A tile's picture: the item's thumbnail, or what stands in for one.
export function StudioThumbnail(props: Props) {
  const { t } = useTranslation('studio')
  const frameClassName = `grid place-items-center overflow-hidden bg-surface-secondary ${props.className ?? ''}`

  if (props.thumbnail.kind === 'image') {
    return <div className={frameClassName}>
      <img
        src={getStudioAssetContentUrl(props.itemId, props.thumbnail.assetId)}
        alt=''
        loading='lazy'
        className='size-full object-contain'
      />
    </div>
  }

  if (props.thumbnail.kind === 'working') {
    return <div className={frameClassName}>
      <div className='flex flex-col items-center gap-2 text-xs opacity-70'>
        <Spinner size='sm' />
        <span>{t('grid.working')}</span>
      </div>
    </div>
  }

  return <div className={frameClassName}>
    <div className='flex flex-col items-center gap-2 text-xs opacity-50'>
      <LuImageOff className='size-6' aria-hidden />
      <span>{t('grid.noImage')}</span>
    </div>
  </div>
}
