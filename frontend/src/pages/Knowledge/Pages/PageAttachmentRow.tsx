// Copyright © 2026 Jalapeno Labs

import type { PageAttachment } from '../../../api/routes/pageRoutes'

// Core
import { useTranslation } from 'react-i18next'

// User interface
import { Button, Tooltip } from '@heroui/react'
import { LuFile, LuX } from 'react-icons/lu'
import { CopyButton } from '../../../components/CopyButton'
import { ImagePreview } from '../../../components/ImagePreview'

// Misc
import { DateText } from '../../../format/DateText'
import { formatBytes } from '../../../format/numbers'
import { useUserNames } from '../../../hooks/useUserNames'
import { attachmentMarkdown } from './pageAttachmentPresentation'
import { pageAgentName } from './pageHistoryPresentation'

type Props = {
  attachment: PageAttachment
  isRemoving: boolean
  // Unset on a page that changes no further.
  onRemove?: () => void
}

// One file: a small image or an icon, its name, which downloads it, then its size, who added it, and
// when. Copying its Markdown is how a file attached here reaches the page's text.
export function PageAttachmentRow(props: Props) {
  const { t } = useTranslation([ 'knowledge', 'common' ])
  const userNames = useUserNames()
  const { attachment } = props
  const who = userNames[attachment.createdBy] ?? t('common:table.someone')
  const size = formatBytes(attachment.sizeBytes)

  return <li className='flex items-center gap-3 py-2'>
    <div className='grid size-10 shrink-0 place-items-center overflow-hidden rounded-md bg-surface-secondary'>
      {attachment.isImage
        ? <ImagePreview src={attachment.url} alt={attachment.name}>
          <img src={attachment.url} alt='' loading='lazy' className='size-10 object-cover' />
        </ImagePreview>
        : <LuFile className='size-5 opacity-60' aria-hidden />}
    </div>
    <div className='min-w-0 flex-1'>
      {/* A plain anchor: the browser fetches this, never the router, and a name this long truncates. */}
      <a
        href={attachment.url}
        download={attachment.name}
        aria-label={t('attachments.download', { name: attachment.name })}
        title={attachment.name}
        className='block truncate text-sm text-link hover:underline'
      >{
          attachment.name
        }</a>
      <div className='flex flex-wrap items-center gap-x-2 text-xs opacity-60'>
        <span>{
          attachment.agent
            ? t('attachments.addedByAgent', { size, who, agent: pageAgentName(attachment.agent, t) })
            : t('attachments.added', { size, who })
        }</span>
        <span aria-hidden>·</span>
        <DateText value={attachment.createdAt} />
      </div>
    </div>
    <CopyButton
      value={attachmentMarkdown(attachment)}
      label={t('attachments.copyMarkdown', { name: attachment.name })}
    />
    {props.onRemove && <Tooltip delay={400}>
      <Button
        isIconOnly
        size='sm'
        variant='ghost'
        aria-label={t('attachments.remove', { name: attachment.name })}
        isPending={props.isRemoving}
        onPress={props.onRemove}
      >
        <LuX className='size-4' aria-hidden />
      </Button>
      <Tooltip.Content>{t('attachments.remove', { name: attachment.name })}</Tooltip.Content>
    </Tooltip>}
  </li>
}
