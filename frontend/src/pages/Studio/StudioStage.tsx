// Copyright © 2026 Jalapeno Labs

import type { ModelViewerElement } from '@google/model-viewer'
import type { StudioItem } from '../../api/routes/studioRoutes'
import type { AnnotationSource } from './annotation'
import type { StageSubject } from './studioAssets'

// Core
import { lazy, Suspense, useEffect, useMemo, useRef, useState } from 'react'
import { useTranslation } from 'react-i18next'

// Redux
import { useAppDispatch, useAppSelector } from '../../store/hooks'
import { selectStudioItemAssets } from '../../store/studioAssetsSlice'
import { studioItemUpserted } from '../../store/studioItemsSlice'

// User interface
import { Button, Spinner, toast, Tooltip } from '@heroui/react'
import { LuPencilLine, LuPin, LuPinOff } from 'react-icons/lu'
import { AnnotateModal } from './AnnotateModal'
import { StudioDownloads } from './StudioDownloads'
import { StudioFilmstrip } from './StudioFilmstrip'

// Misc
import { getApiErrorMessage } from '../../api/errors'
import { getStudioAssetContentUrl, updateStudioItem } from '../../api/routes/studioRoutes'
import { getDefaultStageSubject, getSubjectVersions, groupStudioAssets } from './studioAssets'

// three.js is most of the viewer's weight, so it loads with the first model shown rather
// than with the app.
const StudioModelViewer = lazy(async () => {
  const module = await import('./StudioModelViewer')
  return { default: module.StudioModelViewer }
})

type Props = {
  item: StudioItem
  // A deleted item is shown read-only.
  isReadOnly: boolean
  // The agent is at work, so an empty stage says files are coming.
  isWorking: boolean
}

// The left column: the image or model shown, the filmstrip to pick another or an older
// version, and the downloads.
export function StudioStage(props: Props) {
  const { t } = useTranslation([ 'studio', 'common' ])
  const dispatch = useAppDispatch()
  const item = props.item
  const assets = useAppSelector((state) => selectStudioItemAssets(state, item.id))
  const groups = useMemo(() => groupStudioAssets(assets), [ assets ])
  const viewerRef = useRef<ModelViewerElement>(null)

  // What the user picked; until then, or once it is gone, the stage picks.
  const [ chosenSubject, setChosenSubject ] = useState<StageSubject | null>(null)
  // The version picked in the filmstrip; null follows the newest as turns deliver.
  const [ chosenVersionId, setChosenVersionId ] = useState<string | null>(null)
  const [ isPinning, setIsPinning ] = useState(false)
  const [ annotationSource, setAnnotationSource ] = useState<AnnotationSource | null>(null)

  // A frozen model view lives in a blob URL for as long as the drawing over it is open.
  useEffect(() => {
    const frozenViewUrl = annotationSource?.capture
      ? annotationSource.imageUrl
      : null
    return () => {
      if (frozenViewUrl) {
        URL.revokeObjectURL(frozenViewUrl)
      }
    }
  }, [ annotationSource ])

  const isChosenSubjectPresent = chosenSubject?.kind === 'image'
    ? groups.images.some((image) => image.path === chosenSubject.path)
    : groups.models.some((model) => model.stem === chosenSubject?.stem)
  const subject = chosenSubject && isChosenSubjectPresent
    ? chosenSubject
    : getDefaultStageSubject(groups, item.thumbnailAssetId)

  if (!subject) {
    return <div className='grid h-full place-items-center p-6'>
      <div className='text-center text-sm opacity-70'>
        {props.isWorking && <Spinner className='compact mx-auto' />}
        <p>{
          props.isWorking
            ? t('stage.emptyWorking')
            : t('stage.empty')
        }</p>
      </div>
    </div>
  }

  const versions = getSubjectVersions(groups, subject)
  const shownAsset = versions.find((version) => version.id === chosenVersionId) ?? versions[0]
  const isPinned = Boolean(shownAsset) && item.pinnedAssetId === shownAsset.id

  async function togglePin() {
    if (!shownAsset) {
      console.debug('StudioStage was asked to pin with nothing shown', { itemId: item.id })
      return
    }

    setIsPinning(true)
    try {
      const response = await updateStudioItem(item.id, {
        thumbnailAssetId: isPinned
          ? null
          : shownAsset.id,
      })
      dispatch(studioItemUpserted(response.item))
      toast.success(isPinned
        ? t('stage.toasts.unpinned')
        : t('stage.toasts.pinned'))
    }
    catch (error) {
      const message = getApiErrorMessage(error)
      if (!message) {
        console.debug('StudioStage failed to change the thumbnail', { error, itemId: item.id })
      }
      toast.danger(t('stage.toasts.pinFailed'), {
        description: message ?? t('common:errors.unexpected'),
      })
    }
    finally {
      setIsPinning(false)
    }
  }

  // An image is drawn on as it is. A model's view is frozen first, with the camera's orbit,
  // so the drawing and the agent both see exactly what was on screen.
  async function openAnnotation() {
    if (!shownAsset) {
      console.debug('StudioStage was asked to annotate with nothing shown', { itemId: item.id })
      return
    }

    if (shownAsset.kind === 'image') {
      setAnnotationSource({
        imageUrl: getStudioAssetContentUrl(item.id, shownAsset.id),
        sourceAssetId: shownAsset.id,
      })
      return
    }

    const viewer = viewerRef.current
    try {
      if (!viewer?.loaded) {
        throw new Error('The model has not finished loading')
      }
      const capture = await viewer.toBlob({ mimeType: 'image/png' })
      setAnnotationSource({
        imageUrl: URL.createObjectURL(capture),
        sourceAssetId: shownAsset.id,
        cameraOrbit: viewer.getCameraOrbit().toString(),
        capture,
      })
    }
    catch (error) {
      console.debug('StudioStage could not freeze the model\'s view', { error, itemId: item.id })
      toast.danger(t('viewer.captureFailed'))
    }
  }

  function changeSubject(nextSubject: StageSubject) {
    setChosenSubject(nextSubject)
    setChosenVersionId(null)
  }

  const pinLabel = isPinned
    ? t('stage.unpin')
    : t('stage.pin')

  return <div className='flex h-full flex-col'>
    <div className='level shrink-0 gap-2 px-3 py-2'>
      {/* Only a model without a `.glb` shows no file. */}
      <p className='min-w-0 truncate text-sm font-medium'>{
        shownAsset?.artifactPath ?? t('stage.model')
      }</p>
      <div className='flex shrink-0 items-center gap-2'>
        {isPinned && <span className='text-xs opacity-60'>{t('stage.pinned')}</span>}
        {shownAsset && !props.isReadOnly && <Tooltip delay={300}>
          <Button
            size='sm'
            variant='outline'
            onPress={() => void openAnnotation()}
          >
            <LuPencilLine className='size-4' aria-hidden />
            <span>{t('stage.annotate')}</span>
          </Button>
          <Tooltip.Content>
            <span>{t('stage.annotateHint')}</span>
          </Tooltip.Content>
        </Tooltip>}
        {shownAsset?.kind === 'image' && !props.isReadOnly && <Tooltip delay={300}>
          <Button
            isIconOnly
            size='sm'
            variant='ghost'
            aria-label={pinLabel}
            isPending={isPinning}
            onPress={() => void togglePin()}
          >
            {isPinned
              ? <LuPinOff className='size-4' aria-hidden />
              : <LuPin className='size-4' aria-hidden />}
          </Button>
          <Tooltip.Content>
            <span>{pinLabel}</span>
          </Tooltip.Content>
        </Tooltip>}
      </div>
    </div>

    <div className='relative min-h-0 flex-1 bg-surface-secondary'>
      {shownAsset?.kind === 'image' && <img
        src={getStudioAssetContentUrl(item.id, shownAsset.id)}
        alt={shownAsset.name}
        className='size-full object-contain'
      />}

      {shownAsset?.kind === 'model' && <Suspense fallback={<div className='grid size-full place-items-center'>
        <Spinner />
      </div>}>
        <StudioModelViewer
          src={getStudioAssetContentUrl(item.id, shownAsset.id)}
          name={shownAsset.name}
          viewerRef={viewerRef}
        />
      </Suspense>}

      {!shownAsset && <div className='grid size-full place-items-center p-6'>
        <p className='max-w-sm text-center text-sm opacity-70'>{t('stage.noPreview')}</p>
      </div>}
    </div>

    <StudioFilmstrip
      itemId={item.id}
      groups={groups}
      subject={subject}
      onSubjectChange={changeSubject}
      versions={versions}
      shownVersionId={shownAsset?.id ?? null}
      onVersionChange={setChosenVersionId}
    />

    <StudioDownloads
      itemId={item.id}
      groups={groups.downloads}
    />

    {annotationSource && <AnnotateModal
      itemId={item.id}
      source={annotationSource}
      onClose={() => setAnnotationSource(null)}
    />}
  </div>
}
