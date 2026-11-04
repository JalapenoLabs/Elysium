// Copyright © 2026 Jalapeno Labs

// Core
import i18next from 'i18next'

// Misc
import { formatDateTime } from '../../../format/dates'

// How long a downloaded file's `blob:` URL is kept: long enough for any browser to start the
// download, short enough to hold nothing for long.
const BLOB_RELEASE_DELAY_MS = 1000

type BackupCodesFile = {
  filename: string
  contents: string
}

type Options = {
  // What the person calls their authenticator app, such as "Bitwarden"; empty when unnamed.
  appName: string
  // Whose codes they are: the account's email.
  account: string
  codes: string[]
  createdAt: Date
}

// The text file of backup codes a person downloads, named for the app and the moment it was
// made, so a file found months later says which app it backs up and how recent it is. The time
// is the viewer's own; the filename's stamp sorts, the text's reads.
export function buildBackupCodesFile(options: Options): BackupCodesFile {
  const appName = options.appName.trim() || i18next.t('security.backupCodes.file.unnamedApp', { ns: 'auth' })
  const lines = [
    i18next.t('security.backupCodes.file.heading', { ns: 'auth' }),
    i18next.t('security.backupCodes.file.app', { ns: 'auth', name: appName }),
    i18next.t('security.backupCodes.file.account', { ns: 'auth', account: options.account }),
    i18next.t('security.backupCodes.file.created', { ns: 'auth', date: formatDateTime(options.createdAt) }),
    i18next.t('security.backupCodes.file.usage', { ns: 'auth' }),
    '',
    ...options.codes,
    '',
  ]

  return {
    filename: `elysium-backup-codes-${slugify(appName)}-${fileStamp(options.createdAt)}.txt`,
    contents: lines.join('\n'),
  }
}

// Saves `file` to the person's downloads in one click. A `blob:` link, which the web app's
// Content Security Policy allows, and nothing leaves the browser.
export function downloadBackupCodesFile(file: BackupCodesFile) {
  const url = URL.createObjectURL(new Blob([ file.contents ], { type: 'text/plain;charset=utf-8' }))
  const link = document.createElement('a')
  link.href = url
  link.download = file.filename
  link.click()
  // Released once the browser has taken the file: revoking within the click can cancel the
  // download in some browsers.
  window.setTimeout(() => URL.revokeObjectURL(url), BLOB_RELEASE_DELAY_MS)
}


// "Bitwarden (work)" as "bitwarden-work": what file systems and download bars show well.
function slugify(name: string) {
  const slug = name
    .normalize('NFKD')
    .toLowerCase()
    .replace(/[^a-z0-9]+/g, '-')
    .replace(/^-+|-+$/g, '')
  return slug || 'app'
}

// "2026-11-04-1530", in the viewer's time zone.
function fileStamp(date: Date) {
  const pad = (value: number) => String(value).padStart(2, '0')
  const day = `${date.getFullYear()}-${pad(date.getMonth() + 1)}-${pad(date.getDate())}`
  return `${day}-${pad(date.getHours())}${pad(date.getMinutes())}`
}
