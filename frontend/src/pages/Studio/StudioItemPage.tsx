// Copyright © 2026 Jalapeno Labs

// Core
import { useTranslation } from 'react-i18next'
import { useParams } from 'react-router'

// Redux
import { useAppSelector } from '../../store/hooks'
import { selectStudioItemById } from '../../store/studioItemsSlice'

// User interface
import { Link, Spinner } from '@heroui/react'
import { StudioItemWorkspace } from './StudioItemWorkspace'

// Misc
import { useStudioItemLoader } from '../../hooks/useServerData'
import { UrlTree } from '../../urls'

// `/studio/:itemId`: one item, live or deleted. Loads it, then hands it to the workspace,
// which can rely on it being there.
export function StudioItemPage() {
  const { t } = useTranslation('studio')
  const itemId = useParams().itemId ?? ''
  const status = useStudioItemLoader(itemId)
  const item = useAppSelector((state) => selectStudioItemById(state, itemId))

  if (item) {
    return <StudioItemWorkspace item={item} />
  }

  if (status === 'loading') {
    return <div className='grid h-full place-items-center'>
      <Spinner />
    </div>
  }

  // Failing to load is most often an item deleted for good, which answers 404; one deleted
  // for good while shown leaves Redux with the load still standing.
  return <div className='grid h-full place-items-center p-6'>
    <div className='text-center'>
      <p className='compact text-sm opacity-70'>{t('item.missing')}</p>
      <Link href={UrlTree.studio} className='text-link'>{t('item.back')}</Link>
    </div>
  </div>
}
