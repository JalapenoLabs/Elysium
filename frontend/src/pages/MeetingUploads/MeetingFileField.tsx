// Copyright © 2026 Jalapeno Labs

import type { DragEvent, RefCallback } from 'react'
import type { MeetingUploadFileKind, MeetingUploadPolicy } from '../../api/routes/meetingUploadRoutes'
import type { FileRefusal } from './meetingUpload'

// Core
import { useRef, useState } from 'react'
import { useTranslation } from 'react-i18next'

// User interface
import { Button, Label, Tooltip } from '@heroui/react'
import { LuFileAudio, LuFileText, LuUpload, LuX } from 'react-icons/lu'

// Misc
import { formatBytes } from '../../format/numbers'
import { describeFileRefusal } from './meetingUpload'

const iconByKind = {
  recording: LuFileAudio,
  transcript: LuFileText,
} as const satisfies Record<MeetingUploadFileKind, typeof LuFileText>

const chooseKeyByKind = {
  recording: 'upload.fields.chooseRecording',
  transcript: 'upload.fields.chooseTranscript',
} as const

const labelKeyByKind = {
  recording: 'upload.fields.recording',
  transcript: 'upload.fields.transcript',
} as const

type Props = {
  kind: MeetingUploadFileKind
  policy: MeetingUploadPolicy
  file: File | null
  onChange: (file: File | null) => void
  isDisabled: boolean
  // Why the API refused the file chosen here.
  errorMessage?: string
  // Given to the button that chooses a file, for the page to start there.
  chooseRef?: RefCallback<HTMLElement>
}

// One file of a meeting's upload, chosen from the picker or dropped on the field. A file the
// meeting would refuse, by its type or its size, is refused here with the reason, before a byte of
// it is sent; the API refuses it again whichever way it arrives.
export function MeetingFileField(props: Props) {
  const { t } = useTranslation('meetings')
  const inputRef = useRef<HTMLInputElement>(null)
  const [ refusal, setRefusal ] = useState<string | null>(null)
  const [ isDropping, setIsDropping ] = useState(false)
  const taken = props.policy[props.kind]
  const Icon = iconByKind[props.kind]
  const id = `meeting-upload-${props.kind}`
  const kinds = taken.extensions.map((extension) => extension.toUpperCase()).join(', ')

  function choose(file: File | undefined) {
    if (!file) {
      console.debug('MeetingFileField: no file was chosen', { kind: props.kind })
      return
    }
    const refused = describeFileRefusal(file, props.kind, props.policy)
    if (refused) {
      const messages = {
        type: () => t('upload.refusals.type', { extensions: kinds }),
        empty: () => t('upload.refusals.empty', { name: file.name }),
        tooLarge: () => t('upload.refusals.tooLarge', { limit: formatBytes(taken.maxBytes) }),
      } as const satisfies Record<FileRefusal, () => string>
      setRefusal(messages[refused]())
      return
    }
    setRefusal(null)
    props.onChange(file)
  }

  function drop(event: DragEvent<HTMLDivElement>) {
    event.preventDefault()
    setIsDropping(false)
    if (props.isDisabled) {
      return
    }
    choose(event.dataTransfer.files[0])
  }

  const hint = t('upload.fields.hint', { extensions: kinds, limit: formatBytes(taken.maxBytes) })
  const message = refusal ?? props.errorMessage
  // A file held over the field lights it, so it is clear where it drops.
  const dropTone = isDropping
    ? 'border-accent bg-accent-soft'
    : 'border-separator'

  return <div className='flex min-w-0 flex-col gap-2'>
    <Label htmlFor={id}>{t(labelKeyByKind[props.kind])}</Label>
    <div
      className={`flex min-h-16 items-center gap-3 rounded-xl border border-dashed px-4 py-3 ${dropTone}
        transition-colors motion-reduce:transition-none`}
      onDragOver={(event) => {
        event.preventDefault()
        setIsDropping(!props.isDisabled)
      }}
      onDragLeave={() => setIsDropping(false)}
      onDrop={drop}
    >
      <Icon className='size-6 shrink-0 opacity-70' aria-hidden />
      {props.file
        ? <div className='flex min-w-0 flex-1 items-center gap-2'>
          <div className='min-w-0 flex-1'>
            <p className='truncate text-sm font-medium'>{props.file.name}</p>
            <p className='text-xs opacity-70'>{formatBytes(props.file.size)}</p>
          </div>
          <Tooltip delay={300}>
            <Button
              isIconOnly
              size='sm'
              variant='ghost'
              aria-label={t('upload.fields.remove')}
              isDisabled={props.isDisabled}
              onPress={() => {
                setRefusal(null)
                props.onChange(null)
              }}
            >
              <LuX className='size-4' aria-hidden />
            </Button>
            <Tooltip.Content>{t('upload.fields.remove')}</Tooltip.Content>
          </Tooltip>
        </div>
        : <div className='flex min-w-0 flex-1 flex-wrap items-center justify-between gap-2'>
          <p className='text-xs opacity-70'>{hint}</p>
          <Button
            ref={props.chooseRef}
            size='sm'
            variant='outline'
            isDisabled={props.isDisabled}
            onPress={() => inputRef.current?.click()}
          >
            <LuUpload className='size-4' aria-hidden />
            <span>{t(chooseKeyByKind[props.kind])}</span>
          </Button>
        </div>}
      <input
        ref={inputRef}
        id={id}
        type='file'
        className='hidden'
        accept={taken.extensions.map((extension) => `.${extension}`).join(',')}
        onChange={(event) => {
          choose(event.currentTarget.files?.[0])
          // Clearing lets the same file be chosen again after removing it.
          event.currentTarget.value = ''
        }}
      />
    </div>
    {message && <p className='text-xs text-danger' role='alert'>{message}</p>}
  </div>
}
