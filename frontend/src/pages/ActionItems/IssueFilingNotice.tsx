// Copyright © 2026 Jalapeno Labs

import type { ActionItem, IssueFiling } from '../../api/routes/actionItemRoutes'

// Core
import { useTranslation } from 'react-i18next'

// Redux
import { useAppDispatch } from '../../store/hooks'
import { actionItemUpserted } from '../../store/actionItemsSlice'

// User interface
import { Button, Spinner } from '@heroui/react'
import { ReportIssueButton } from '../../components/ReportIssueButton'

// Misc
import { cancelActionItemIssueFiling } from '../../api/routes/actionItemRoutes'
import { useConfirm } from '../../hooks/useConfirm'
import { providerLabelKeys } from './linkPresentation'
import { toast } from '../../toast/toast'

type Props = {
  item: ActionItem
  filing: IssueFiling
}

// The issue an item is owed and has not been filed yet, shown like a link's pending write: where
// it files, how its tries have gone, and a way to stop trying. The watcher files it and links it
// as the item's primary once its tracker takes it.
export function IssueFilingNotice(props: Props) {
  const { t } = useTranslation([ 'actionItems', 'common' ])
  const dispatch = useAppDispatch()
  const confirm = useConfirm()
  const provider = t(providerLabelKeys[props.filing.provider])

  function cancel() {
    confirm({
      title: t('links.filing.cancelTitle'),
      message: t('links.filing.cancelBody', { provider }),
      tone: 'danger',
      confirmText: t('links.pending.cancel'),
      onConfirm: async () => {
        try {
          await cancelActionItemIssueFiling(props.item.id)
        }
        catch (error) {
          console.debug('IssueFilingNotice failed to cancel a filing', { error, itemId: props.item.id })
          toast.danger(t('common:errors.unexpected'))
          throw error
        }
        dispatch(actionItemUpserted({ ...props.item, issueFiling: null }))
        toast.success(t('toasts.writeCancelled'))
      },
    })
  }

  return <div className='compact level gap-3 rounded-lg bg-surface-secondary px-3 py-2 text-xs'>
    <div className='flex min-w-0 items-start gap-2'>
      <Spinner size='sm' className='mt-0.5 shrink-0' />
      <div className='min-w-0'>
        <p className='font-medium'>
          {t('links.filing.label', { provider, destination: props.filing.destination })}
          <span className='ml-2 opacity-60'>{
            props.filing.attempts
              ? t('links.pending.tried', { count: props.filing.attempts })
              : t('links.pending.queued')
          }</span>
        </p>
        {props.filing.lastError && <p className='break-words text-warning'>{
          t('links.pending.lastError', { error: props.filing.lastError })
        }</p>}
      </div>
    </div>
    <div className='flex shrink-0 items-center gap-1'>
      {props.filing.lastError && <ReportIssueButton
        isIconOnly
        subject={{
          kind: 'error',
          area: `action_items.issue_filing.${props.filing.provider}`,
          message: props.filing.lastError,
        }}
      />}
      {!props.item.deletedAt && <Button size='sm' variant='tertiary' onPress={cancel}>
        <span>{t('links.pending.cancel')}</span>
      </Button>}
    </div>
  </div>
}
