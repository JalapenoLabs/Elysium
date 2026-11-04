// Copyright © 2026 Jalapeno Labs

// Core
import { useTranslation } from 'react-i18next'
import { useParams } from 'react-router'

// Redux
import { useAppSelector } from '../../store/hooks'
import { selectActionItemById } from '../../store/actionItemsSlice'

// User interface
import { Link, Spinner } from '@heroui/react'
import { ActionItemView } from './ActionItemView'

// Misc
import { useActionItemLoader } from '../../hooks/useServerData'
import { UrlTree } from '../../urls'

// `/action-items/:itemId`: loads the item, then hands it to `ActionItemView`, which can rely
// on it being there.
export function ActionItemPage() {
  const { t } = useTranslation('actionItems')
  const { itemId = '' } = useParams()
  const status = useActionItemLoader(itemId)
  const item = useAppSelector((state) => selectActionItemById(state, itemId))

  if (item) {
    return <ActionItemView item={item} />
  }

  return <div className='container'>{
    status === 'loading'
      ? <div className='grid place-items-center py-16'>
        <Spinner />
      </div>
      : <div className='py-16 text-center'>
        <p className='compact text-sm opacity-70'>{t('item.notFound')}</p>
        <Link href={UrlTree.actionItemsAll} className='text-link'>{t('item.backToList')}</Link>
      </div>
  }</div>
}
