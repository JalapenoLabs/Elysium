// Copyright © 2026 Jalapeno Labs

import type { DragEvent } from 'react'
import type { Drive, FileEntry } from '../proto/generated/storage_pb'

// Core
import { useEffect, useState } from 'react'
import { useTranslation } from 'react-i18next'

// Redux
import { useAppSelector } from '../store/hooks'

// UI
import { Button, Select, SelectItem, Tooltip } from '@heroui/react'
import { LuChevronRight, LuFolderPlus, LuHardDrive, LuRefreshCw, LuUpload } from 'react-icons/lu'
import { EmptyState } from '../components/states/EmptyState'
import { ErrorState } from '../components/states/ErrorState'
import { LoadingState } from '../components/states/LoadingState'
import { FileList } from './FileList'

// Utility
import { HTTPError } from 'ky'

// Misc
import { useConfirm } from '../hooks/useConfirm'
import { usePrompt } from '../hooks/usePrompt'
import { listDrives } from '../api/routes/driveRoutes'
import {
  createFolder,
  deleteEntry,
  getDownloadUrl,
  listFiles,
  moveEntry,
  uploadFile,
} from '../api/routes/fileRoutes'
import { joinPath, parentPath, pathSegments } from './paths'
import { bindRevealListener } from './revealBus'

// The Files workspace panel (docs/storage.md): browse the active project's
// Drives, navigate folders, upload (button or drag-and-drop straight onto the
// listing), download, create folders, rename, and delete. The provider is the
// source of truth; every operation round-trips through the backend and the
// listing reloads from what the provider reports.
export function FilesPanel() {
  const { t } = useTranslation()
  const confirm = useConfirm()
  const prompt = usePrompt()

  const activeProjectId = useAppSelector((state) => state.projects.activeProjectId)

  const [ drives, setDrives ] = useState<Drive[]>([])
  const [ driveId, setDriveId ] = useState<string | null>(null)
  const [ path, setPath ] = useState('')
  const [ entries, setEntries ] = useState<FileEntry[]>([])
  const [ activeIndex, setActiveIndex ] = useState(0)
  const [ isLoading, setIsLoading ] = useState(true)
  const [ isBusy, setIsBusy ] = useState(false)
  const [ isDropTarget, setIsDropTarget ] = useState(false)
  const [ errorMessage, setErrorMessage ] = useState<string | null>(null)
  const [ noticeMessage, setNoticeMessage ] = useState<string | null>(null)

  // The Drive roster follows the active project; DnD lands on the default
  // Drive first (docs/storage.md).
  useEffect(() => {
    if (!activeProjectId) {
      return
    }
    setIsLoading(true)
    listDrives(activeProjectId)
      .then((response) => {
        setDrives(response.drives)
        const defaultDrive = response.drives.find((drive) => drive.isDefault) ?? response.drives[0]
        setDriveId(defaultDrive?.id?.value ?? null)
        setPath('')
        if (!response.drives.length) {
          setIsLoading(false)
        }
      })
      .catch((error) => {
        console.debug('drive roster unavailable for the files panel', { error })
        setErrorMessage(t('drive.files.loadFailed'))
        setIsLoading(false)
      })
  }, [ activeProjectId ])

  async function loadListing(targetDriveId: string, targetPath: string) {
    setIsLoading(true)
    setErrorMessage(null)
    try {
      const listing = await listFiles(targetDriveId, targetPath)
      // The FS provider currently returns the listed root itself as a
      // nameless entry; drop such junk rows defensively (flagged backend-side).
      const entries = listing.entries.filter((entry) => {
        if (!entry.name) {
          console.debug('dropping a nameless listing entry', { path: entry.path })
          return false
        }
        return true
      })
      setEntries(entries)
      setActiveIndex(0)
    }
    catch (error) {
      console.debug('listing unavailable', { error })
      setErrorMessage(t('drive.files.loadFailed'))
    }
    finally {
      setIsLoading(false)
    }
  }

  useEffect(() => {
    if (driveId) {
      loadListing(driveId, path)
    }
  }, [ driveId, path ])

  // Search results (and future surfaces) send this panel to a location.
  useEffect(() => {
    return bindRevealListener((target) => {
      setDriveId(target.driveId)
      setPath(target.path)
    })
  }, [])

  // Wraps a mutation with busy handling and a listing reload.
  async function run(action: () => Promise<unknown>) {
    if (!driveId) {
      console.debug('files panel action with no drive selected, ignoring')
      return
    }
    setIsBusy(true)
    setNoticeMessage(null)
    try {
      await action()
      await loadListing(driveId, path)
    }
    catch (error) {
      if (error instanceof HTTPError && error.response.status === 413) {
        setNoticeMessage(t('drive.files.uploadTooLarge'))
      }
      else {
        console.debug('file operation failed', { error })
        setNoticeMessage(t('drive.files.operationFailed'))
      }
    }
    finally {
      setIsBusy(false)
    }
  }

  function onOpen(entry: FileEntry) {
    if (entry.isDir) {
      setPath(entry.path)
    }
    else {
      onDownload(entry)
    }
  }

  function onDownload(entry: FileEntry) {
    if (!driveId) {
      console.debug('download requested with no drive selected, ignoring')
      return
    }
    // An anchor navigation lets the browser stream the download itself.
    const anchor = document.createElement('a')
    anchor.href = getDownloadUrl(driveId, entry.path)
    anchor.download = entry.name
    anchor.click()
  }

  function onRename(entry: FileEntry) {
    prompt({
      title: t('drive.files.renameTitle', { name: entry.name }),
      defaultValue: entry.name,
      inputProps: { label: t('drive.files.renameLabel') },
      validate: (value) => {
        if (!value.trim() || value.includes('/')) {
          return t('drive.files.nameInvalid')
        }
        return null
      },
      confirmLabel: t('drive.files.renameAction'),
      onConfirm: (value) => run(() => {
        const destination = joinPath(parentPath(entry.path), value.trim())
        return moveEntry(driveId ?? '', entry.path, destination)
      }),
    })
  }

  function onDelete(entry: FileEntry) {
    confirm({
      title: t('drive.files.delete'),
      message: t('drive.files.deleteConfirm', { name: entry.name }),
      confirmLabel: t('drive.files.delete'),
      confirmColor: 'danger',
      onConfirm: () => run(() => deleteEntry(driveId ?? '', entry.path)),
    })
  }

  function onNewFolder() {
    prompt({
      title: t('drive.files.newFolderTitle'),
      inputProps: { label: t('drive.files.newFolderLabel') },
      validate: (value) => {
        if (!value.trim() || value.includes('/')) {
          return t('drive.files.nameInvalid')
        }
        return null
      },
      confirmLabel: t('drive.files.newFolderAction'),
      onConfirm: (value) => run(() => createFolder(driveId ?? '', joinPath(path, value.trim()))),
    })
  }

  async function uploadFiles(files: File[]) {
    if (!files.length) {
      console.debug('upload requested with no files, ignoring')
      return
    }
    await run(async () => {
      for (const file of files) {
        await uploadFile(driveId ?? '', joinPath(path, file.name), file)
      }
    })
  }

  function onPickFiles() {
    const input = document.createElement('input')
    input.type = 'file'
    input.multiple = true
    input.onchange = () => {
      uploadFiles([ ...(input.files ?? []) ])
    }
    input.click()
  }

  function onDrop(event: DragEvent<HTMLDivElement>) {
    event.preventDefault()
    setIsDropTarget(false)
    uploadFiles([ ...event.dataTransfer.files ])
  }

  const segments = pathSegments(path)

  return <div
    className={`flex h-full flex-col p-3 ${isDropTarget ? 'bg-primary/5' : ''}`}
    onDragOver={(event) => {
      event.preventDefault()
      setIsDropTarget(true)
    }}
    onDragLeave={() => setIsDropTarget(false)}
    onDrop={onDrop}
  >
    <div className='compact level items-end'>
      <Select
        size='sm'
        className='max-w-56'
        label={t('drive.files.driveLabel')}
        startContent={<LuHardDrive className='h-4 w-4 opacity-60' />}
        selectedKeys={driveId ? [ driveId ] : []}
        onChange={(event) => {
          if (event.target.value) {
            setDriveId(event.target.value)
            setPath('')
          }
        }}
      >
        { drives.map((drive) =>
          <SelectItem key={drive.id?.value}>{ drive.name }</SelectItem>,
        ) }
      </Select>
      <div className='flex items-center gap-1'>
        <Tooltip content={t('drive.files.refresh')}>
          <div>
            <Button
              isIconOnly
              size='sm'
              variant='light'
              aria-label={t('drive.files.refresh')}
              isDisabled={isBusy || !driveId}
              onPress={() => {
                if (driveId) {
                  loadListing(driveId, path)
                }
              }}
            >
              <LuRefreshCw className='h-4 w-4 opacity-60' />
            </Button>
          </div>
        </Tooltip>
        <Button
          size='sm'
          variant='bordered'
          startContent={<LuFolderPlus className='h-4 w-4' />}
          isDisabled={isBusy || !driveId}
          onPress={onNewFolder}
        >
          <span>{ t('drive.files.newFolderAction') }</span>
        </Button>
        <Button
          size='sm'
          color='primary'
          variant='flat'
          startContent={<LuUpload className='h-4 w-4' />}
          isDisabled={isBusy || !driveId}
          isLoading={isBusy}
          onPress={onPickFiles}
        >
          <span>{ t('drive.files.upload') }</span>
        </Button>
      </div>
    </div>

    {/* Breadcrumbs: the Drive root plus each folder on the way down. */}
    <div className='compact flex flex-wrap items-center gap-1 text-sm'>
      <button
        type='button'
        className='opacity-70 hover:opacity-100'
        onClick={() => setPath('')}
      >
        { t('drive.files.root') }
      </button>
      { segments.map((segment) => (
        <span key={segment.path} className='flex items-center gap-1'>
          <LuChevronRight className='h-3 w-3 opacity-40' />
          <button
            type='button'
            className='opacity-70 hover:opacity-100'
            onClick={() => setPath(segment.path)}
          >
            { segment.name }
          </button>
        </span>
      )) }
    </div>

    { noticeMessage && (
      <p className='compact text-sm text-status-error'>{ noticeMessage }</p>
    ) }

    <div className='min-h-0 flex-1 overflow-y-auto'>
      { isLoading && <LoadingState label={t('drive.files.loading')} /> }

      { !isLoading && errorMessage && (
        <ErrorState
          title={errorMessage}
          onRetry={() => {
            if (driveId) {
              loadListing(driveId, path)
            }
          }}
        />
      ) }

      { !isLoading && !errorMessage && !drives.length && (
        <EmptyState
          icon={<LuHardDrive />}
          title={t('drive.files.noDrives')}
          description={t('drive.files.noDrivesHint')}
        />
      ) }

      { !isLoading && !errorMessage && Boolean(drives.length) && !entries.length && (
        <EmptyState
          icon={<LuUpload />}
          title={t('drive.files.emptyFolder')}
          description={t('drive.files.emptyFolderHint')}
        />
      ) }

      { !isLoading && !errorMessage && entries.length
        ? <FileList
          entries={entries}
          activeIndex={activeIndex}
          onActiveIndexChange={setActiveIndex}
          onOpen={onOpen}
          onDownload={onDownload}
          onRename={onRename}
          onDelete={onDelete}
          isBusy={isBusy}
        />
        : null }
    </div>
  </div>
}
