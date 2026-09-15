// Copyright © 2026 Jalapeno Labs

// Core
import { useEffect, useRef, useState } from 'react'
import { useTranslation } from 'react-i18next'

// User interface
import { Button, Description, Label } from '@heroui/react'
import { LuImagePlus, LuTrash2 } from 'react-icons/lu'
import { ProjectCover } from './ProjectCover'

// Misc
import { PROJECT_COVER_ACCEPTED_TYPES } from '../../constants'
import { getCoverFileErrorKey } from './projectCover'

type Props = {
  name: string
  // The chosen cover, or null for none.
  file: File | null
  onChange: (file: File | null) => void
}

// Picks a new project's cover and previews it in the same frame tiles use. Nothing uploads
// here: the form uploads it after the project is saved, since a new project has no id to
// attach a cover to until then.
export function ProjectCoverField(props: Props) {
  const { t } = useTranslation('projects')
  const inputRef = useRef<HTMLInputElement>(null)
  const [ error, setError ] = useState<string | null>(null)
  const [ previewUrl, setPreviewUrl ] = useState<string | null>(null)

  // An object URL holds the file in memory until it is revoked.
  useEffect(() => {
    const url = props.file
      ? URL.createObjectURL(props.file)
      : null
    setPreviewUrl(url)
    return () => {
      if (url) {
        URL.revokeObjectURL(url)
      }
    }
  }, [ props.file ])

  function onFileChosen(file: File | undefined) {
    if (!file) {
      console.debug('ProjectCoverField: the file picker closed without a file')
      return
    }
    const errorKey = getCoverFileErrorKey(file)
    if (errorKey) {
      setError(t(errorKey))
      return
    }
    setError(null)
    props.onChange(file)
  }

  const hasCover = Boolean(previewUrl)

  return <div className='flex flex-col gap-2'>
    <Label>{t('form.cover')}</Label>
    <ProjectCover
      src={previewUrl}
      fit='fit'
      name={props.name || '?'}
      className='aspect-video w-full rounded-2xl'
    />
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
          props.onChange(null)
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
