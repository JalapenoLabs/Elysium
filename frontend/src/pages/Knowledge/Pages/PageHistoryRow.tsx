// Copyright © 2026 Jalapeno Labs

import type { KnowledgePage } from '../../../api/routes/pageRoutes'
import type { PageHistoryRow as Row } from './pageHistoryPresentation'

// Core
import { useTranslation } from 'react-i18next'

// User interface
import { buttonVariants, Checkbox, Chip, Link } from '@heroui/react'
import { LuBookOpen, LuDot } from 'react-icons/lu'
import { RestoreAttachmentButton } from './RestoreAttachmentButton'
import { RestoreVersionButton } from './RestoreVersionButton'

// Misc
import { DateText } from '../../../format/DateText'
import { getPageCompareUrl, getPageVersionUrl } from '../../../urls'

type Props = {
  page: KnowledgePage
  row: Row
  // What happened, whole, in the reader's words.
  sentence: string
  isPicked: boolean
  // Unset on a page with no second version to compare with.
  onPickChange?: (isPicked: boolean) => void
}

// One entry of a page's history. A version opens on what it changed, which is what most people come
// for, through a link stretched over its row; its checkbox, Read, and Restore sit above that link.
export function PageHistoryRow(props: Props) {
  const { t } = useTranslation('knowledge')
  const { page, row } = props
  const version = row.version
  const when = <div className='text-xs opacity-50'>
    <DateText value={row.entry.createdAt} isFocusable={false} />
  </div>

  if (version === null) {
    // A file taken off stays with its provider, so this is where it comes back from once its toast
    // has gone. One back on the page already has nothing to restore.
    const removed = row.sentence === 'detached'
      && row.attachment
      && !page.attachments.some((attachment) => attachment.id === row.attachment?.id)
      ? row.attachment
      : null

    return <li className='flex flex-wrap items-start gap-x-3 px-3 py-2'>
      <LuDot className='size-5 shrink-0 opacity-50' aria-hidden />
      <div className='min-w-0 flex-1 text-sm'>
        <p className='opacity-70'>{props.sentence}</p>
        {when}
      </div>
      {removed && !page.deletedAt && <RestoreAttachmentButton
        pageId={page.id}
        attachmentId={removed.id}
        name={removed.name}
      />}
    </li>
  }

  const isCurrent = version === page.currentVersion
  const changesHref = version > 1
    ? getPageCompareUrl(page.id, version - 1, version)
    : getPageVersionUrl(page.id, version)

  return <li className='relative flex flex-wrap items-start gap-x-3 rounded-lg px-3 py-2 hover:bg-surface-secondary'>
    {props.onPickChange && <div className='relative z-1 flex h-6 w-5 shrink-0 items-center justify-center'>
      <Checkbox
        aria-label={t('pages.history.pick', { version })}
        isSelected={props.isPicked}
        onChange={props.onPickChange}
      >
        <Checkbox.Content>
          <Checkbox.Control>
            <Checkbox.Indicator />
          </Checkbox.Control>
        </Checkbox.Content>
      </Checkbox>
    </div>}
    <div className='min-w-0 flex-1 text-sm'>
      <div className='flex flex-wrap items-center gap-2'>
        <Link
          href={changesHref}
          className='font-medium text-link no-underline after:absolute after:inset-0 hover:underline'
        >{
            t('pages.history.version', { version })
          }</Link>
        {isCurrent && <Chip size='sm' variant='soft' color='accent'>{t('pages.history.current')}</Chip>}
      </div>
      <p className='opacity-70'>{props.sentence}</p>
      {/* Above the row's link, so hovering it still gives the full timestamp. */}
      <div className='relative z-1 w-fit'>
        {when}
      </div>
    </div>
    <div className='relative z-1 flex basis-full items-center gap-1 pl-8 sm:basis-auto sm:pl-0'>
      <Link
        href={getPageVersionUrl(page.id, version)}
        aria-label={t('pages.history.readLabel', { version })}
        className={buttonVariants({ size: 'sm', variant: 'ghost' })}
      >
        <LuBookOpen className='size-4' aria-hidden />
        <span>{t('pages.history.read')}</span>
      </Link>
      {!isCurrent && !page.deletedAt && <RestoreVersionButton pageId={page.id} version={version} />}
    </div>
  </li>
}
