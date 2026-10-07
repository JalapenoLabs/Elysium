// Copyright © 2026 Jalapeno Labs

import type { StudioDownloadGroup } from './studioAssets'

// Core
import { useTranslation } from 'react-i18next'

// User interface
import { LuDownload } from 'react-icons/lu'

// Misc
import { getStudioAssetContentUrl } from '../../api/routes/studioRoutes'
import { formatStorageBytes } from '../Settings/Storage/storagePresentation'

type Props = {
  itemId: string
  groups: StudioDownloadGroup[]
}

// The newest version of every file, grouped by stem, so a model's `.blend` and `.glb` sit
// together.
export function StudioDownloads(props: Props) {
  const { t, i18n } = useTranslation('studio')

  if (!props.groups.length) {
    return null
  }

  return <details className='shrink-0 border-t border-separator px-3 py-2 text-sm'>
    <summary className='cursor-pointer text-xs font-medium opacity-70'>{t('stage.downloads')}</summary>
    <ul className='mt-2 flex max-h-48 flex-col gap-2 overflow-y-auto'>{
      props.groups.map((group) => <li key={group.stem}>
        <p className='truncate text-xs opacity-60'>{group.stem}</p>
        <ul className='flex flex-wrap gap-x-4 gap-y-1'>{
          group.assets.map((asset) => <li key={asset.id}>
            {/* A plain anchor: the API answers with an attachment, which a client-side
                route change would not follow. */}
            <a
              href={getStudioAssetContentUrl(props.itemId, asset.id, { download: true })}
              download={asset.name}
              aria-label={t('stage.download', { name: asset.name })}
              className='inline-flex items-center gap-1 text-link no-underline hover:underline'
            >
              <LuDownload className='size-3.5' aria-hidden />
              <span>{asset.name}</span>
              <span className='opacity-60'>{formatStorageBytes(asset.sizeBytes, i18n.language)}</span>
            </a>
          </li>)
        }</ul>
      </li>)
    }</ul>
  </details>
}
