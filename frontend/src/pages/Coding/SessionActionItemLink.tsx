// Copyright © 2026 Jalapeno Labs

// Core
import { useTranslation } from 'react-i18next'

// Redux
import { selectActionItemById } from '../../store/actionItemsSlice'
import { useAppSelector } from '../../store/hooks'

// User interface
import { Link } from '@heroui/react'

// Misc
import { useActionItemLoader } from '../../hooks/useServerData'
import { getActionItemViewUrl } from '../../urls'

type Props = {
  actionItemId: string
}

// The action item a session was started from, linked to its page.
export function SessionActionItemLink(props: Props) {
  const { t } = useTranslation('coding')
  useActionItemLoader(props.actionItemId)
  const actionItem = useAppSelector((state) => selectActionItemById(state, props.actionItemId))

  if (!actionItem) {
    return <span className='opacity-60'>{t('conversation.fromItemUnavailable')}</span>
  }

  return <Link
    href={getActionItemViewUrl(actionItem.id)}
    className='text-link no-underline hover:underline'
  >{
    t('conversation.fromItem', { title: actionItem.title })
  }</Link>
}
