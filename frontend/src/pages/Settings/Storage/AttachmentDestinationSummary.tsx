// Copyright © 2026 Jalapeno Labs

import type { StorageLocation } from '../../../api/routes/storageRoutes'

// Core
import { useTranslation } from 'react-i18next'
import useSWR from 'swr'

// User interface
import { buttonVariants, Link } from '@heroui/react'
import { LuPaperclip } from 'react-icons/lu'

// Misc
import { getPageAttachmentPolicy, PAGE_ATTACHMENT_POLICY_KEY } from '../../../api/routes/pageRoutes'
import { UrlTree } from '../../../urls'

type Props = {
  locations: StorageLocation[]
}

// Which location page and meeting attachments go to, beside the locations it is chosen from. Changing it has a
// page of its own, since this page's list is what it is about.
export function AttachmentDestinationSummary(props: Props) {
  const { t } = useTranslation('storage')
  const { data } = useSWR(PAGE_ATTACHMENT_POLICY_KEY, getPageAttachmentPolicy)
  if (!data) {
    return null
  }

  const destination = props.locations.find((location) => location.id === data.policy.destinationId)

  return <div className='mt-4 flex flex-wrap items-center justify-between gap-2 rounded-xl border border-separator p-3'>
    <div className='flex min-w-0 items-center gap-2 text-sm'>
      <LuPaperclip className='size-4 shrink-0 opacity-60' aria-hidden />
      <span>{
        destination
          ? t('attachments.summary', { name: destination.name })
          : t('attachments.summaryNone')
      }</span>
    </div>
    <Link
      href={UrlTree.settingsStorageAttachments}
      className={buttonVariants({ size: 'sm', variant: 'outline' })}
    >
      <span>{t('attachments.change')}</span>
    </Link>
  </div>
}
