// Copyright © 2026 Jalapeno Labs

import type { Selection } from '@heroui/react'
import type { Project, ProjectCoverFit } from '../../api/routes/projectRoutes'

// Core
import { useEffect, useRef, useState } from 'react'
import { useTranslation } from 'react-i18next'

// User interface
import { Button, ToggleButton, ToggleButtonGroup, Tooltip } from '@heroui/react'
import { LuExpand, LuImagePlus, LuShrink, LuUndo2 } from 'react-icons/lu'
import { ProjectCover } from './ProjectCover'

// Misc
import { getProjectCoverUrl, PROJECT_COVER_FITS } from '../../api/routes/projectRoutes'
import { PROJECT_COVER_ACCEPTED_TYPES } from '../../constants'
import { getCoverFileErrorKey } from './projectCover'
import { toast } from '../../toast/toast'

const ADD_COVER_CLASS_NAME = [
  'grid h-28 w-full place-items-center rounded-2xl border-2 border-dashed border-separator text-sm opacity-70',
  'transition hover:opacity-100 focus-visible:ring-2 focus-visible:ring-focus focus-visible:outline-none',
].join(' ')

// The cover's controls stay out of the way until the banner is hovered or holds focus, unless
// a new cover is waiting to be saved, which they offer to undo.
const COVER_CONTROLS_CLASS_NAME = [
  'absolute top-3 right-3 flex items-center gap-2 transition-opacity',
  'group-focus-within:opacity-100 group-hover:opacity-100',
].join(' ')

type Props = {
  project: Project
  // How the cover sits in its frame, as the page's form holds it.
  fit: ProjectCoverFit
  onFitChange: (fit: ProjectCoverFit) => void
  // A new cover picked to replace the current one, uploaded by the page's Save.
  coverFile: File | null
  onCoverFileChange: (file: File | null) => void
}

// The cover across the project page, a field of the page kept by its Save. Double-clicking it,
// or its Change cover button, picks a new image, shown here until it is saved; the Fit and Fill
// switch beside that button changes how every rendering of the cover sits in its frame. Without
// a cover, the whole strip is one button that picks one.
export function ProjectBanner(props: Props) {
  const { t } = useTranslation([ 'projects', 'common' ])
  const inputRef = useRef<HTMLInputElement>(null)
  const [ previewUrl, setPreviewUrl ] = useState<string | null>(null)
  const coverUrl = previewUrl ?? getProjectCoverUrl(props.project)

  // An object URL holds the file in memory until it is revoked.
  useEffect(() => {
    const url = props.coverFile
      ? URL.createObjectURL(props.coverFile)
      : null
    setPreviewUrl(url)
    return () => {
      if (url) {
        URL.revokeObjectURL(url)
      }
    }
  }, [ props.coverFile ])

  function chooseFile() {
    inputRef.current?.click()
  }

  function changeFit(keys: Selection) {
    // The group disallows an empty selection, so one fit is always selected.
    const [ selected ] = keys === 'all'
      ? []
      : [ ...keys ]
    const fit = PROJECT_COVER_FITS.find((candidate) => candidate === selected)
    if (!fit) {
      console.debug('ProjectBanner ignored an unknown cover fit', { selected })
      return
    }
    props.onFitChange(fit)
  }

  function pick(file: File | undefined) {
    if (!file) {
      console.debug('ProjectBanner: the file picker closed without a file')
      return
    }
    const errorKey = getCoverFileErrorKey(file)
    if (errorKey) {
      toast.danger(t('toasts.coverUploadFailed'), { description: t(errorKey) })
      return
    }
    props.onCoverFileChange(file)
  }

  const fileInput = <input
    ref={inputRef}
    type='file'
    accept={PROJECT_COVER_ACCEPTED_TYPES.join(',')}
    className='hidden'
    onChange={(event) => {
      pick(event.currentTarget.files?.[0])
      // Clearing lets the same file be chosen again.
      event.currentTarget.value = ''
    }}
  />

  if (!coverUrl) {
    return <div className='relaxed relative'>
      {fileInput}
      <button
        type='button'
        className={ADD_COVER_CLASS_NAME}
        onClick={chooseFile}
      >
        <span className='flex items-center gap-2'>
          <LuImagePlus className='size-5' aria-hidden />
          <span>{t('page.addCover')}</span>
        </span>
      </button>
    </div>
  }

  const controlsVisibility = props.coverFile
    ? 'opacity-100'
    : 'opacity-0'

  return <div className='group relaxed relative'>
    {fileInput}
    <Tooltip delay={600}>
      <Tooltip.Trigger
        className='block cursor-pointer rounded-2xl'
        onDoubleClick={chooseFile}
      >
        <ProjectCover
          src={coverUrl}
          fit={props.fit}
          name={props.project.name}
          className='h-48 w-full rounded-2xl sm:h-64'
        />
      </Tooltip.Trigger>
      <Tooltip.Content>
        <span>{t('page.coverHint')}</span>
      </Tooltip.Content>
    </Tooltip>
    <div className={`${COVER_CONTROLS_CLASS_NAME} ${controlsVisibility}`}>
      {props.coverFile && <Button
        size='sm'
        variant='secondary'
        onPress={() => props.onCoverFileChange(null)}
      >
        <LuUndo2 className='size-4' aria-hidden />
        <span>{t('page.keepCover')}</span>
      </Button>}
      <ToggleButtonGroup
        size='sm'
        aria-label={t('page.coverFit')}
        selectionMode='single'
        disallowEmptySelection
        selectedKeys={[ props.fit ]}
        onSelectionChange={changeFit}
      >
        <ToggleButton id='fit'>
          <LuShrink className='size-4' aria-hidden />
          <span>{t('page.fit')}</span>
        </ToggleButton>
        <ToggleButton id='fill'>
          <ToggleButtonGroup.Separator />
          <LuExpand className='size-4' aria-hidden />
          <span>{t('page.fill')}</span>
        </ToggleButton>
      </ToggleButtonGroup>
      <Button
        size='sm'
        variant='secondary'
        onPress={chooseFile}
      >
        <LuImagePlus className='size-4' aria-hidden />
        <span>{t('page.changeCover')}</span>
      </Button>
    </div>
  </div>
}
