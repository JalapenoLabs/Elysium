// Copyright © 2026 Jalapeno Labs

import type { DragEvent } from 'react'
import type { KnowledgePage, PageAttachment } from '../../../api/routes/pageRoutes'
import type { usePageAttachmentUpload } from './usePageAttachments'

// Core
import { useRef, useState } from 'react'
import { useTranslation } from 'react-i18next'

// Redux
import { selectIsAdmin } from '../../../store/authSlice'
import { useAppDispatch, useAppSelector } from '../../../store/hooks'
import { pageLoaded } from '../../../store/pagesSlice'

// User interface
import { Button, Link, Spinner } from '@heroui/react'
import { LuPaperclip } from 'react-icons/lu'
import { PageAttachmentRow } from './PageAttachmentRow'

// Misc
import { getApiErrorMessage } from '../../../api/errors'
import { removePageAttachment, restorePageAttachment } from '../../../api/routes/pageRoutes'
import { formatBytes } from '../../../format/numbers'
import { toast } from '../../../toast/toast'
import { UrlTree } from '../../../urls'

type Props = {
  page: KnowledgePage
  // Held by whoever shows this, so the editor beside it shares what is on its way.
  upload: ReturnType<typeof usePageAttachmentUpload>
}

function holdsFiles(event: DragEvent) {
  return event.dataTransfer.types.includes('Files')
}

// A page's files: each with its size, who added it, and when, and somewhere to add more. A file is an
// action, not a field of the page, so each applies at once and makes no version, and removing one
// is undone from its toast, as a label is. Dropping files anywhere on the section attaches them. A
// deleted page changes no further, so it only lists what it holds.
export function PageAttachments(props: Props) {
  const { page, upload } = props
  const { t } = useTranslation([ 'knowledge', 'common' ])
  const dispatch = useAppDispatch()
  const isAdmin = useAppSelector(selectIsAdmin)
  const inputRef = useRef<HTMLInputElement>(null)
  const [ isDropTarget, setIsDropTarget ] = useState(false)
  const [ removingId, setRemovingId ] = useState<string | null>(null)

  const isReadOnly = Boolean(page.deletedAt)
  const canAttach = !isReadOnly && Boolean(upload.policy) && !upload.hasNoLocation

  async function attachAll(files: File[]) {
    for (const file of files) {
      await upload.attach(file)
    }
  }

  async function restore(attachment: PageAttachment) {
    try {
      const response = await restorePageAttachment(page.id, attachment.id)
      dispatch(pageLoaded(response.page))
    }
    catch (error) {
      console.debug('PageAttachments failed to restore a file', { error, pageId: page.id, id: attachment.id })
      toast.danger(t('attachments.restoreFailed', { name: attachment.name }), {
        description: getApiErrorMessage(error),
      })
    }
  }

  async function remove(attachment: PageAttachment) {
    setRemovingId(attachment.id)
    try {
      const response = await removePageAttachment(page.id, attachment.id)
      dispatch(pageLoaded(response.page))
      toast.success(t('attachments.removed', { name: attachment.name }), {
        action: {
          label: t('common:actions.undo'),
          onPress: () => void restore(attachment),
        },
      })
    }
    catch (error) {
      console.debug('PageAttachments failed to remove a file', { error, pageId: page.id, id: attachment.id })
      toast.danger(t('attachments.removeFailed', { name: attachment.name }), {
        description: getApiErrorMessage(error),
      })
    }
    finally {
      setRemovingId(null)
    }
  }

  if (isReadOnly && !page.attachments.length) {
    return null
  }

  const dropProps = canAttach
    ? {
      onDragOver: (event: DragEvent) => {
        if (holdsFiles(event)) {
          event.preventDefault()
          setIsDropTarget(true)
        }
      },
      onDragLeave: (event: DragEvent) => {
        // Moving onto something inside the section is still over it.
        const { relatedTarget } = event
        if (!(relatedTarget instanceof Node) || !event.currentTarget.contains(relatedTarget)) {
          setIsDropTarget(false)
        }
      },
      onDrop: (event: DragEvent) => {
        event.preventDefault()
        setIsDropTarget(false)
        void attachAll([ ...event.dataTransfer.files ])
      },
    }
    : {}

  return <section
    aria-label={t('attachments.label')}
    className={`rounded-xl border p-3 transition-colors ${isDropTarget
      ? 'border-accent bg-surface-secondary'
      : 'border-separator'}`}
    {...dropProps}
  >
    <div className='flex flex-wrap items-center justify-between gap-2'>
      <h2 className='text-sm font-medium'>{t('attachments.label')}</h2>
      {canAttach && <div className='flex flex-wrap items-center gap-2'>
        {upload.policy && <span className='text-xs opacity-60'>{
          isDropTarget
            ? t('attachments.dropHere')
            : t('attachments.hint', { size: formatBytes(upload.policy.maxBytes) })
        }</span>}
        <input
          ref={inputRef}
          id='page-attachment-file'
          type='file'
          multiple
          accept={upload.accept}
          className='hidden'
          aria-label={t('attachments.attach')}
          onChange={(event) => {
            const files = [ ...event.currentTarget.files ?? [] ]
            // Clearing lets the same file be chosen again.
            event.currentTarget.value = ''
            void attachAll(files)
          }}
        />
        <Button size='sm' variant='outline' onPress={() => inputRef.current?.click()}>
          <LuPaperclip className='size-4' aria-hidden />
          <span>{t('attachments.attach')}</span>
        </Button>
      </div>}
    </div>

    {!isReadOnly && upload.hasNoLocation && <p className='mt-1 text-xs opacity-70'>
      <span>{t('attachments.noLocation')}</span>
      {isAdmin && <>
        {' '}
        <Link href={UrlTree.settingsStorageAttachments} className='text-link'>{
          t('attachments.noLocationAdmin')
        }</Link>
      </>}
    </p>}
    {!isReadOnly && upload.hasFailed && !upload.policy && <p className='mt-1 text-xs opacity-70'>{
      t('attachments.unavailable')
    }</p>}

    {Boolean(page.attachments.length) && <ul className='mt-1 divide-y divide-separator'>{
      page.attachments.map((attachment) => <PageAttachmentRow
        key={attachment.id}
        attachment={attachment}
        isRemoving={removingId === attachment.id}
        onRemove={isReadOnly
          ? undefined
          : () => void remove(attachment)}
      />)
    }</ul>}

    {Boolean(upload.uploading.length) && <ul aria-live='polite' className='mt-1'>{
      upload.uploading.map((name, index) => <li
        // The same file can be on its way twice, so its place tells them apart.
        key={`${name}-${index}`}
        className='flex items-center gap-2 py-1 text-sm opacity-70'
      >
        <Spinner size='sm' />
        <span className='truncate'>{t('attachments.uploading', { name })}</span>
      </li>)
    }</ul>}
  </section>
}
