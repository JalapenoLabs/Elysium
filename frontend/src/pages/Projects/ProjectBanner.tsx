// Copyright © 2026 Jalapeno Labs

import type { Project } from '../../api/routes/projectRoutes'

// Core
import { useRef, useState } from 'react'
import { useTranslation } from 'react-i18next'

// Redux
import { useAppDispatch } from '../../store/hooks'
import { projectUpserted } from '../../store/projectsSlice'

// User interface
import { Button, Spinner, Tooltip, toast } from '@heroui/react'
import { LuImagePlus } from 'react-icons/lu'
import { ProjectCover } from './ProjectCover'

// Utility
import { HTTPError } from 'ky'

// Misc
import { getProjectCoverUrl, uploadProjectCover } from '../../api/routes/projectRoutes'
import { PROJECT_COVER_ACCEPTED_TYPES } from '../../constants'
import { getCoverFileErrorKey } from './projectCover'

const ADD_COVER_CLASS_NAME = [
  'grid h-28 w-full place-items-center rounded-2xl border-2 border-dashed border-separator text-sm opacity-70',
  'transition hover:opacity-100 focus-visible:ring-2 focus-visible:ring-focus focus-visible:outline-none',
].join(' ')

// Out of the way until the banner is hovered or holds focus.
const CHANGE_COVER_CLASS_NAME = [
  'absolute top-3 right-3 opacity-0 transition-opacity',
  'group-focus-within:opacity-100 group-hover:opacity-100',
].join(' ')

type Props = {
  project: Project
}

// The cover across the top of the project page. Double-clicking it, or its Change cover
// button, picks a new image and uploads it at once. Without a cover, the whole strip is
// one button that adds one.
export function ProjectBanner(props: Props) {
  const { t } = useTranslation([ 'projects', 'common' ])
  const dispatch = useAppDispatch()
  const inputRef = useRef<HTMLInputElement>(null)
  const [ isUploading, setIsUploading ] = useState(false)
  const coverUrl = getProjectCoverUrl(props.project)

  function chooseFile() {
    if (isUploading) {
      return
    }
    inputRef.current?.click()
  }

  async function upload(file: File | undefined) {
    if (!file) {
      console.debug('ProjectBanner: the file picker closed without a file')
      return
    }
    const errorKey = getCoverFileErrorKey(file)
    if (errorKey) {
      toast.danger(t('toasts.coverUploadFailed'), { description: t(errorKey) })
      return
    }

    setIsUploading(true)
    try {
      const response = await uploadProjectCover(props.project.id, file)
      dispatch(projectUpserted(response.project))
      toast.success(t('toasts.coverUpdated'))
    }
    catch (error) {
      // The API explains a refusal, such as a file that is not really an image.
      const description = error instanceof HTTPError && error.response.status === 400
        ? t('form.errors.coverUnsupported')
        : t('common:errors.unexpected')
      console.debug('ProjectBanner failed to upload the cover', { error, projectId: props.project.id })
      toast.danger(t('toasts.coverUploadFailed'), { description })
    }
    finally {
      setIsUploading(false)
    }
  }

  const fileInput = <input
    ref={inputRef}
    type='file'
    accept={PROJECT_COVER_ACCEPTED_TYPES.join(',')}
    className='hidden'
    onChange={(event) => {
      void upload(event.currentTarget.files?.[0])
      // Clearing lets the same file be chosen again.
      event.currentTarget.value = ''
    }}
  />

  const uploadingOverlay = isUploading && <div
    className='absolute inset-0 grid place-items-center rounded-2xl bg-surface/70'
  >
    <span className='flex items-center gap-2 text-sm'>
      <Spinner size='sm' />
      <span>{t('page.uploadingCover')}</span>
    </span>
  </div>

  if (!coverUrl) {
    return <div className='relaxed relative'>
      {fileInput}
      <button
        type='button'
        className={ADD_COVER_CLASS_NAME}
        disabled={isUploading}
        onClick={chooseFile}
      >
        <span className='flex items-center gap-2'>
          <LuImagePlus className='size-5' aria-hidden />
          <span>{t('page.addCover')}</span>
        </span>
      </button>
      {uploadingOverlay}
    </div>
  }

  return <div className='group relaxed relative'>
    {fileInput}
    <Tooltip delay={600}>
      <Tooltip.Trigger
        className='block cursor-pointer rounded-2xl'
        onDoubleClick={chooseFile}
      >
        <ProjectCover
          src={coverUrl}
          name={props.project.name}
          className='h-48 w-full rounded-2xl sm:h-64'
        />
      </Tooltip.Trigger>
      <Tooltip.Content>
        <span>{t('page.coverHint')}</span>
      </Tooltip.Content>
    </Tooltip>
    <Button
      size='sm'
      variant='secondary'
      className={CHANGE_COVER_CLASS_NAME}
      isDisabled={isUploading}
      onPress={chooseFile}
    >
      <LuImagePlus className='size-4' aria-hidden />
      <span>{t('page.changeCover')}</span>
    </Button>
    {uploadingOverlay}
  </div>
}
