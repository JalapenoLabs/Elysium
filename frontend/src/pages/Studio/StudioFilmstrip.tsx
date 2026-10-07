// Copyright © 2026 Jalapeno Labs

import type { StudioAsset } from '../../api/routes/studioRoutes'
import type { StageSubject, StudioAssetGroups } from './studioAssets'

// Core
import { useTranslation } from 'react-i18next'

// User interface
import { LuBox } from 'react-icons/lu'

// Misc
import { getStudioAssetContentUrl } from '../../api/routes/studioRoutes'

const FRAME_CLASS_NAME = [
  'flex w-24 shrink-0 cursor-pointer flex-col gap-1 rounded-lg p-1 text-left text-xs',
  'transition-colors hover:bg-surface-tertiary',
  'focus-visible:ring-2 focus-visible:ring-focus focus-visible:outline-none',
].join(' ')

const SELECTED_FRAME_CLASS_NAME = 'bg-surface-tertiary ring-2 ring-accent'

const PICTURE_CLASS_NAME = 'grid aspect-square w-full place-items-center overflow-hidden rounded-md bg-surface'

type Props = {
  itemId: string
  groups: StudioAssetGroups
  subject: StageSubject
  onSubjectChange: (subject: StageSubject) => void
  // The subject's versions, newest first, and the one shown.
  versions: StudioAsset[]
  shownVersionId: string | null
  onVersionChange: (assetId: string) => void
}

// Two strips under the stage: every model and image the item holds, then the versions of
// the one shown, so a render can be compared turn by turn.
export function StudioFilmstrip(props: Props) {
  const { t, i18n } = useTranslation('studio')
  const dateFormatter = new Intl.DateTimeFormat(i18n.language, { dateStyle: 'short', timeStyle: 'short' })

  function frameClassName(isSelected: boolean) {
    if (isSelected) {
      return `${FRAME_CLASS_NAME} ${SELECTED_FRAME_CLASS_NAME}`
    }
    return FRAME_CLASS_NAME
  }

  return <div className='shrink-0 border-t border-separator px-3 py-2'>
    <p className='mb-1 text-xs font-medium opacity-60'>{t('stage.files')}</p>
    <ul className='flex gap-2 overflow-x-auto pb-1'>
      {props.groups.models.map((model) => {
        const isSelected = props.subject.kind === 'model' && props.subject.stem === model.stem
        return <li key={`model:${model.stem}`}>
          <button
            type='button'
            aria-pressed={isSelected}
            className={frameClassName(isSelected)}
            onClick={() => props.onSubjectChange({ kind: 'model', stem: model.stem })}
          >
            <span className={PICTURE_CLASS_NAME}>
              <LuBox className='size-8 opacity-60' aria-hidden />
            </span>
            <span className='truncate'>{model.stem.split('/').at(-1)}</span>
          </button>
        </li>
      })}
      {props.groups.images.map((image) => {
        const isSelected = props.subject.kind === 'image' && props.subject.path === image.path
        return <li key={`image:${image.path}`}>
          <button
            type='button'
            aria-pressed={isSelected}
            className={frameClassName(isSelected)}
            onClick={() => props.onSubjectChange({ kind: 'image', path: image.path })}
          >
            <span className={PICTURE_CLASS_NAME}>
              <img
                src={getStudioAssetContentUrl(props.itemId, image.versions[0].id)}
                alt=''
                loading='lazy'
                className='size-full object-contain'
              />
            </span>
            <span className='truncate'>{image.versions[0].name}</span>
          </button>
        </li>
      })}
    </ul>

    {props.versions.length > 1 && <>
      <p className='mt-2 mb-1 text-xs font-medium opacity-60'>{t('stage.versions')}</p>
      <ul className='flex gap-2 overflow-x-auto pb-1'>{
        props.versions.map((version, index) => {
          const isSelected = version.id === props.shownVersionId
          const label = index === 0
            ? t('stage.latest')
            : dateFormatter.format(new Date(version.createdAt))
          return <li key={version.id}>
            <button
              type='button'
              aria-pressed={isSelected}
              aria-label={t('stage.version', { date: dateFormatter.format(new Date(version.createdAt)) })}
              className={frameClassName(isSelected)}
              onClick={() => props.onVersionChange(version.id)}
            >
              <span className={PICTURE_CLASS_NAME}>{
                version.kind === 'image'
                  ? <img
                    src={getStudioAssetContentUrl(props.itemId, version.id)}
                    alt=''
                    loading='lazy'
                    className='size-full object-contain'
                  />
                  : <LuBox className='size-8 opacity-60' aria-hidden />
              }</span>
              <span className='truncate'>{label}</span>
            </button>
          </li>
        })
      }</ul>
    </>}
  </div>
}
