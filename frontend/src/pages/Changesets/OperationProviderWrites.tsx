// Copyright © 2026 Jalapeno Labs

import type { ActionItemLink, PendingWrite } from '../../api/routes/actionItemRoutes'

// Core
import { useTranslation } from 'react-i18next'

// Redux
import { useAppSelector } from '../../store/hooks'
import { selectActionItemLinks } from '../../store/actionItemLinksSlice'

// Misc
import { useActionItemLinksLoader } from '../../hooks/useServerData'
import { providerLabelKeys } from '../ActionItems/linkPresentation'

type Props = {
  itemId: string
  // The comment whose post to show, or none for the closes a resolve owes.
  commentId: string | null
}

// The provider writes an applied operation still owes: the post of its comment, or the
// closes of its item's issues. Each waits on its link until the watcher lands it, showing
// the provider's last answer; once landed it is no longer listed. Read from the item's links,
// which the event stream keeps current.
export function OperationProviderWrites(props: Props) {
  const { t } = useTranslation([ 'changesets', 'actionItems' ])
  useActionItemLinksLoader(props.itemId)
  const links = useAppSelector((state) => selectActionItemLinks(state, props.itemId))

  const owed: { link: ActionItemLink, write: PendingWrite }[] = []
  for (const link of links) {
    for (const write of link.pendingWrites) {
      const isThisOperations = props.commentId
        ? write.commentId === props.commentId
        : write.kind === 'close'
      if (isThisOperations) {
        owed.push({ link, write })
      }
    }
  }
  if (!owed.length) {
    return null
  }

  return <ul className='mt-2 flex flex-col gap-1 text-sm text-warning'>{
    owed.map(({ link, write }) => {
      const provider = t(providerLabelKeys[link.provider], { ns: 'actionItems' })
      return <li key={write.id}>{write.lastError
        ? t('review.providerFailed', { provider, error: write.lastError })
        : t('review.providerWaiting', { provider })}</li>
    })
  }</ul>
}
