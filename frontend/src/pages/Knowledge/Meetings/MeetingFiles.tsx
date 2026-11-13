// Copyright © 2026 Jalapeno Labs

import type { MeetingAttachmentPolicy } from '../../../api/routes/meetingAttachmentRoutes'
import type { MeetingAttachment } from '../../../api/routes/meetingRoutes'
import type { AttachingFile } from './useMeetingChanges'

// Core
import { useRef, useState } from 'react'
import { useTranslation } from 'react-i18next'

// Redux
import { useAppSelector } from '../../../store/hooks'
import { selectUserEntities } from '../../../store/usersSlice'

// User interface
import { Button, Chip, Label, ProgressBar, Tooltip } from '@heroui/react'
import { LuDownload, LuFile, LuPaperclip, LuTrash2 } from 'react-icons/lu'

// Misc
import { DateText } from '../../../format/DateText'
import { formatMediaPosition } from '../../../format/durations'
import { formatBytes, formatNumber } from '../../../format/numbers'
import { orderAttachments } from './meetingAttachments'
import { authorOf } from './meetingReviewPresentation'

// What a file's text is, as its row says: being read, searched, or neither.
const textStateKeys = {
  pending: 'review.files.textPending',
  read: 'review.files.textRead',
  none: 'review.files.textNone',
  unreadable: 'review.files.textUnreadable',
} as const

type Props = {
  attachments: MeetingAttachment[] | null
  hasFailed: boolean
  policy: MeetingAttachmentPolicy | null
  attaching: AttachingFile[]
  // Attaches files chosen or dropped; null where the meeting takes no change.
  onAttach: ((files: File[]) => void) | null
  // Takes one off with its pins, which its toast's Undo puts back; null where the meeting takes no
  // change.
  onDetach: ((attachment: MeetingAttachment) => void) | null
  // A frame chosen: the player goes to it, and its words are selected.
  onSelectFrame: (attachment: MeetingAttachment) => void
}

// The meeting's files and frames in its lane (docs/meeting-review.md, Attachments and frames): the
// frames in the order they were shown, each its picture, moment, and caption, then the files, each
// downloading as it is, with what is on its way. A file dropped on the section is attached, as one
// chosen is.
export function MeetingFiles(props: Props) {
  const { t } = useTranslation([ 'meetings', 'common' ])
  const users = useAppSelector(selectUserEntities)
  const pickerRef = useRef<HTMLInputElement>(null)
  const [ isDropping, setIsDropping ] = useState(false)
  const { frames, files } = orderAttachments(props.attachments ?? [])
  const total = frames.length + files.length
  const { onAttach } = props
  const canAttach = Boolean(onAttach && props.policy?.destinationId)

  function byline(attachment: MeetingAttachment) {
    const author = authorOf(attachment.createdBy, attachment.agent, users)
    return author.name ?? t('common:table.someone')
  }

  let notice = null
  if (props.hasFailed) {
    notice = <p className='text-sm text-danger'>{t('review.files.loadError')}</p>
  }
  else if (props.policy && !props.policy.destinationId) {
    notice = <p className='text-sm opacity-70'>{t('review.files.noDestination')}</p>
  }
  else if (props.attachments && !total && !props.attaching.length) {
    notice = <p className='text-sm opacity-70'>{t('review.files.empty')}</p>
  }

  return <section
    aria-label={t('review.files.title')}
    className={[
      'rounded-xl px-1 transition-colors',
      isDropping
        ? 'bg-accent-soft outline-2 outline-dashed outline-accent'
        : '',
    ].join(' ')}
    onDragOver={(event) => {
      if (!canAttach || !event.dataTransfer.types.includes('Files')) {
        return
      }
      event.preventDefault()
      setIsDropping(true)
    }}
    onDragLeave={() => setIsDropping(false)}
    onDrop={(event) => {
      setIsDropping(false)
      if (!canAttach || !onAttach || !event.dataTransfer.files.length) {
        return
      }
      event.preventDefault()
      onAttach([ ...event.dataTransfer.files ])
    }}
  >
    {/* In a narrow lane Attach files goes under the title rather than breaking it. */}
    <div className='compact flex flex-wrap items-center justify-between gap-x-2 gap-y-1'>
      <p className='flex items-center gap-2 whitespace-nowrap text-xs font-semibold uppercase opacity-60'>
        <span>{t('review.files.title')}</span>
        {Boolean(total) && <Chip size='sm' variant='soft'>{formatNumber(total)}</Chip>}
      </p>
      {onAttach && <>
        <Tooltip delay={300} isDisabled={canAttach}>
          <Tooltip.Trigger>
            <div>
              <Button size='sm' variant='ghost' isDisabled={!canAttach} onPress={() => pickerRef.current?.click()}>
                <LuPaperclip className='size-4' aria-hidden />
                <span>{t('review.files.attach')}</span>
              </Button>
            </div>
          </Tooltip.Trigger>
          <Tooltip.Content className='max-w-xs break-normal'>
            <span>{t('review.files.noDestination')}</span>
          </Tooltip.Content>
        </Tooltip>
        <input
          ref={pickerRef}
          id='meeting-attach-files'
          type='file'
          multiple
          hidden
          onChange={(event) => {
            const chosen = [ ...event.currentTarget.files ?? [] ]
            // The same file can be chosen again once this one is on its way.
            event.currentTarget.value = ''
            if (chosen.length) {
              onAttach(chosen)
            }
          }}
        />
      </>}
    </div>
    {notice}
    {props.policy && canAttach && <p className='compact text-xs opacity-60'>{
      t('review.files.limit', { size: formatBytes(props.policy.maxBytes) })
    }</p>}

    {Boolean(props.attaching.length) && <ul className='compact flex flex-col gap-2'>{
      props.attaching.map((attaching) => <li key={attaching.key} className='text-sm'>
        <ProgressBar
          value={Math.round(attaching.progress * 100)}
          className='w-full'
        >
          <Label className='truncate'>{t('review.files.attaching', { name: attaching.name })}</Label>
          <ProgressBar.Output />
          <ProgressBar.Track>
            <ProgressBar.Fill />
          </ProgressBar.Track>
        </ProgressBar>
      </li>)
    }</ul>}

    {Boolean(frames.length) && <ul className='compact grid grid-cols-2 gap-2'>{
      frames.map((frame) => <li key={frame.id} className='group relative min-w-0'>
        <button
          type='button'
          className='flex w-full flex-col gap-1 rounded-lg border border-border p-1 text-left hover:border-accent'
          onClick={() => props.onSelectFrame(frame)}
        >
          {frame.address && <img
            src={frame.address}
            alt={frame.caption ?? t('review.files.frameAlt')}
            loading='lazy'
            className='aspect-video w-full rounded bg-black object-contain'
          />}
          <span className='text-xs opacity-60'>{
            frame.meetingMs === null
              ? byline(frame)
              : `${formatMediaPosition(frame.meetingMs)} · ${byline(frame)}`
          }</span>
          <span className='line-clamp-2 text-xs'>{frame.caption}</span>
        </button>
        {props.onDetach && <DetachButton
          label={t('review.files.detachFrame')}
          className={[
            'absolute right-2 top-2 bg-overlay opacity-0',
            'group-hover:opacity-100 group-focus-within:opacity-100 pointer-coarse:opacity-100',
          ].join(' ')}
          onPress={() => props.onDetach?.(frame)}
        />}
      </li>)
    }</ul>}

    {Boolean(files.length) && <ul className='compact flex flex-col gap-1'>{
      files.map((file) => <li key={file.id} className='flex items-center gap-2 rounded-lg px-1 py-1 hover:bg-default'>
        {/* A proven image shows itself; any other file is a download. */}
        {file.displaysInline && file.address
          ? <img src={file.address} alt='' loading='lazy' className='size-8 shrink-0 rounded object-cover' />
          : <LuFile className='size-4 shrink-0 opacity-60' aria-hidden />}
        <div className='min-w-0 flex-1'>
          {file.address
            ? <a href={file.address} download className='block truncate text-sm text-link hover:underline'>{
              file.fileName
            }</a>
            : <span className='block truncate text-sm'>{file.fileName}</span>}
          <span className='block truncate text-xs opacity-60'>
            {formatBytes(file.sizeBytes)}
            <span aria-hidden> · </span>
            {file.address
              ? t(textStateKeys[file.textState])
              : t('review.files.storing')}
            <span aria-hidden> · </span>
            {byline(file)}
            <span aria-hidden> · </span>
            <DateText value={file.createdAt} />
          </span>
        </div>
        {file.address && <Tooltip delay={300}>
          <a
            href={file.address}
            download
            aria-label={t('review.files.download', { name: file.fileName })}
            className='grid size-8 shrink-0 place-items-center rounded-lg hover:bg-default-hover'
          >
            <LuDownload className='size-4' aria-hidden />
          </a>
          <Tooltip.Content>{t('review.files.download', { name: file.fileName })}</Tooltip.Content>
        </Tooltip>}
        {props.onDetach && <DetachButton
          label={t('review.files.detachFile', { name: file.fileName })}
          onPress={() => props.onDetach?.(file)}
        />}
      </li>)
    }</ul>}
  </section>
}

type DetachProps = {
  label: string
  className?: string
  onPress: () => void
}

// Takes a file or frame off at once; its toast's Undo puts it back, so it asks nothing first.
function DetachButton(props: DetachProps) {
  return <Tooltip delay={300}>
    <Button
      isIconOnly
      size='sm'
      variant='ghost'
      aria-label={props.label}
      className={props.className}
      onPress={props.onPress}
    >
      <LuTrash2 className='size-4' aria-hidden />
    </Button>
    <Tooltip.Content>{props.label}</Tooltip.Content>
  </Tooltip>
}
