// Copyright © 2026 Jalapeno Labs

import type { ActionItemLink, PendingWrite } from '../../api/routes/actionItemRoutes'

// Core
import { useTranslation } from 'react-i18next'

// Redux
import { useAppDispatch } from '../../store/hooks'
import { actionItemLinkUpserted } from '../../store/actionItemLinksSlice'

// User interface
import { Button, Link, Spinner, toast } from '@heroui/react'

// Misc
import { cancelActionItemLinkWrite } from '../../api/routes/actionItemRoutes'
import { useConfirm } from '../../hooks/useConfirm'
import { getJiraDoneTransitionsUrl } from '../../urls'
import { describePendingWrite, providerLabelKeys } from './linkPresentation'

type Props = {
  link: ActionItemLink
  // A deleted item's writes are shown but not cancelled from its page.
  isReadOnly: boolean
}

// The provider writes a link still owes: each with how its tries have gone, and a way to
// stop trying. The watcher retries every pass until one lands or is cancelled.
export function PendingWrites(props: Props) {
  const { t } = useTranslation([ 'actionItems', 'common' ])
  const dispatch = useAppDispatch()
  const confirm = useConfirm()
  const provider = t(providerLabelKeys[props.link.provider])

  if (!props.link.pendingWrites.length) {
    return null
  }

  function cancel(write: PendingWrite) {
    confirm({
      title: t('links.pending.cancelTitle'),
      message: t('links.pending.cancelBody', { provider }),
      tone: 'danger',
      confirmText: t('links.pending.cancel'),
      onConfirm: async () => {
        try {
          await cancelActionItemLinkWrite(props.link.actionItemId, props.link.id, write.id)
        }
        catch (error) {
          console.debug('PendingWrites failed to cancel a write', { error, writeId: write.id })
          toast.danger(t('common:errors.unexpected'))
          throw error
        }
        dispatch(actionItemLinkUpserted({
          ...props.link,
          pendingWrites: props.link.pendingWrites.filter((pending) => pending.id !== write.id),
        }))
        toast.success(t('toasts.writeCancelled'))
      },
    })
  }

  // A Jira close that has failed is most often a project with several done statuses, which
  // is chosen on the site's settings.
  const doneTransitionsUrl = props.link.provider === 'jira'
    ? getJiraDoneTransitionsUrl(
      props.link.credentialId,
      props.link.key.slice(0, props.link.key.lastIndexOf('-')),
    )
    : null

  return <ul className='mt-2 flex flex-col gap-2'>{
    props.link.pendingWrites.map((write) => {
      const description = describePendingWrite(write)
      return <li key={write.id} className='level gap-3 rounded-lg bg-surface-secondary px-3 py-2 text-xs'>
        <div className='flex min-w-0 items-start gap-2'>
          <Spinner size='sm' className='mt-0.5 shrink-0' />
          <div className='min-w-0'>
            <p className='font-medium'>
              {t(description.labelKey)}
              <span className='ml-2 opacity-60'>{
                description.attempts === null
                  ? t('links.pending.queued')
                  : t('links.pending.tried', { count: description.attempts })
              }</span>
            </p>
            {description.lastError && <p className='break-words text-warning'>{
              t('links.pending.lastError', { error: description.lastError })
            }</p>}
            {description.lastError && write.kind === 'close' && doneTransitionsUrl && <Link
              href={doneTransitionsUrl}
              className='text-link'
            >{t('links.pending.chooseDone')}</Link>}
          </div>
        </div>
        {!props.isReadOnly && <Button size='sm' variant='tertiary' onPress={() => cancel(write)}>
          <span>{t('links.pending.cancel')}</span>
        </Button>}
      </li>
    })
  }</ul>
}
