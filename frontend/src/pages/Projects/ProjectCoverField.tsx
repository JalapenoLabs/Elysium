// Copyright © 2026 Jalapeno Labs

// Core
import { useEffect, useRef, useState } from 'react'
import { useTranslation } from 'react-i18next'

// User interface
import { Button, Description, Label } from '@heroui/react'
import { LuImagePlus, LuTrash2 } from 'react-icons/lu'
import { ProjectCover } from './ProjectCover'

// Misc
import { PROJECT_COVER_ACCEPTED_TYPES, PROJECT_COVER_MAX_BYTES } from '../../constants'

// What the form will do with the cover when it saves.
export type CoverChange =
  | { kind: 'keep' }
  | { kind: 'replace', file: File }
  | { kind: 'remove' }

type Props = {
  // The saved cover's URL, or null when the project has none or does not exist yet.
  savedUrl: string | null
  name: string
  change: CoverChange
  onChange: (change: CoverChange) => void
}

// Picks a cover and previews it in the same frame tiles use. Nothing uploads here: the
// form sends the change after the project itself is saved, since a new project has no id
// to attach a cover to until then.
export function ProjectCoverField(props: Props) {
  const { t } = useTranslation('projects')
  const inputRef = useRef<HTMLInputElement>(null)
  const [ error, setError ] = useState<string | null>(null)
  const [ previewUrl, setPreviewUrl ] = useState<string | null>(null)

  const pendingFile = props.change.kind === 'replace'
    ? props.change.file
    : null

  // An object URL holds the file in memory until it is revoked.
  useEffect(() => {
    const url = pendingFile
      ? URL.createObjectURL(pendingFile)
      : null
    setPreviewUrl(url)
    return () => {
      if (url) {
        URL.revokeObjectURL(url)
      }
    }
  }, [ pendingFile ])

  function onFileChosen(file: File | undefined) {
    if (!file) {
      console.debug('ProjectCoverField: the file picker closed without a file')
      return
    }
    if (!PROJECT_COVER_ACCEPTED_TYPES.includes(file.type)) {
      setError(t('form.errors.coverUnsupported'))
      return
    }
    if (file.size > PROJECT_COVER_MAX_BYTES) {
      setError(t('form.errors.coverTooLarge'))
      return
    }
    setError(null)
    props.onChange({ kind: 'replace', file })
  }

  const shownUrl = props.change.kind === 'remove'
    ? null
    : previewUrl ?? props.savedUrl
  const hasCover = Boolean(shownUrl)

  return <div className='flex flex-col gap-2'>
    <Label>{t('form.cover')}</Label>
    <ProjectCover src={shownUrl} name={props.name || '?'} />
    <input
      ref={inputRef}
      type='file'
      accept={PROJECT_COVER_ACCEPTED_TYPES.join(',')}
      className='hidden'
      onChange={(event) => {
        onFileChosen(event.currentTarget.files?.[0])
        // Clearing lets the same file be chosen again after removing it.
        event.currentTarget.value = ''
      }}
    />
    <div className='flex flex-wrap gap-2'>
      <Button size='sm' variant='outline' onPress={() => inputRef.current?.click()}>
        <LuImagePlus className='size-4' aria-hidden />
        <span>{
          hasCover
            ? t('form.replaceCover')
            : t('form.chooseCover')
        }</span>
      </Button>
      {hasCover && <Button
        size='sm'
        variant='ghost'
        onPress={() => {
          setError(null)
          props.onChange(props.savedUrl
            ? { kind: 'remove' }
            : { kind: 'keep' })
        }}
      >
        <LuTrash2 className='size-4' aria-hidden />
        <span>{t('form.removeCover')}</span>
      </Button>}
    </div>
    {error
      ? <p className='text-xs text-danger' role='alert'>{error}</p>
      : <Description className='text-xs'>{t('form.coverHint')}</Description>}
  </div>
}
