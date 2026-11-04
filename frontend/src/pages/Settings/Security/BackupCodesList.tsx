// Copyright © 2026 Jalapeno Labs

import type { LookupCode } from './securityPresentation'

// Core
import { useTranslation } from 'react-i18next'

// Redux
import { useAppSelector } from '../../../store/hooks'
import { selectMe } from '../../../store/authSlice'

// User interface
import { Button } from '@heroui/react'
import { LuDownload } from 'react-icons/lu'
import { CopyButton } from '../../../components/CopyButton'

// Misc
import { buildBackupCodesFile, downloadBackupCodesFile } from './backupCodesFile'

type Props = {
  codes: LookupCode[]
  // The authenticator app they back up, named in the downloaded file.
  appName: string
  // Called once the file is saved, for a page that waits for it.
  onDownloaded?: () => void
  // Download is the step's one action, as while 2FA is added; beside another primary action, such
  // as confirming new codes, it stays outlined.
  isDownloadPrimary?: boolean
}

// Backup codes as a grid, with buttons to copy them and to download them as one file. Used
// codes stay in place, marked, and are left out of what is copied or saved.
export function BackupCodesList(props: Props) {
  const { t } = useTranslation('auth')
  const me = useAppSelector(selectMe)
  const unused: string[] = []
  for (const entry of props.codes) {
    if (entry.code) {
      unused.push(entry.code)
    }
  }

  function download() {
    downloadBackupCodesFile(buildBackupCodesFile({
      appName: props.appName,
      account: me?.user.email ?? '',
      codes: unused,
      createdAt: new Date(),
    }))
    props.onDownloaded?.()
  }

  return <div>
    <ul className='compact grid grid-cols-2 gap-2 sm:grid-cols-4'>{
      props.codes.map((entry, index) => <li
        key={entry.code ?? `used-${index}`}
        className='rounded-md bg-surface-secondary px-3 py-2 text-center font-mono text-sm'
      >{
          entry.code ?? <span className='opacity-50'>{t('security.lookup.used')}</span>
        }</li>)
    }</ul>
    <div className='level-left flex-wrap'>
      <Button
        size='sm'
        variant={props.isDownloadPrimary
          ? 'primary'
          : 'outline'}
        onPress={download}
      >
        <LuDownload className='size-4' aria-hidden />
        <span>{t('security.backupCodes.download')}</span>
      </Button>
      <CopyButton value={unused.join('\n')} label={t('security.backupCodes.copy')} />
    </div>
  </div>
}
