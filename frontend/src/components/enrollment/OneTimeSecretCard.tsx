// Copyright © 2026 Jalapeno Labs

// UI
import { Button } from '@heroui/react'

// The one-time reveal of a single secret string (an SSH private key, an access
// token). Same show-once discipline as RecoveryCodesCard, but for one value
// rather than a code list: it is displayed exactly once at creation, so this
// offers copy, an optional download-as-file, and a deliberate acknowledgement
// before the value is gone for good. Presentational; copy and the download
// filename are passed in so each flow words and names it.
type Props = {
  title: string
  subtitle: string
  secret: string
  copyLabel: string
  actionLabel: string
  onDone: () => void
  // When set, offers a download of the secret under this filename. A filename
  // is an identifier, so it stays English regardless of locale.
  downloadLabel?: string
  downloadFilename?: string
  isBusy?: boolean
}

export function OneTimeSecretCard(props: Props) {
  function onDownload() {
    if (!props.downloadFilename) {
      console.debug('OneTimeSecretCard download requested without a filename, ignoring')
      return
    }
    const blob = new Blob([ `${props.secret}\n` ], { type: 'text/plain' })
    const url = URL.createObjectURL(blob)
    const anchor = document.createElement('a')
    anchor.href = url
    anchor.download = props.downloadFilename
    anchor.click()
    URL.revokeObjectURL(url)
  }

  return <div>
    <h2 className='text-xl'>
      <strong>{ props.title }</strong>
    </h2>
    <p className='mt-2 text-sm opacity-80'>{ props.subtitle }</p>

    <pre className={
      'mt-4 max-h-48 overflow-auto whitespace-pre-wrap break-all rounded-xl bg-content2 p-4 font-mono text-xs'
    }>{
      props.secret
    }</pre>

    <div className='mt-3 level'>
      <Button
        className='w-full'
        variant='flat'
        onPress={() => {
          // Best-effort: clipboard is unavailable over plain HTTP / in some embeds.
          navigator.clipboard?.writeText(props.secret).catch((error) => {
            console.debug('clipboard write failed', { error })
          })
        }}
      >
        <span>{ props.copyLabel }</span>
      </Button>
      { props.downloadLabel && props.downloadFilename
        ? <Button className='w-full' variant='flat' onPress={onDownload}>
          <span>{ props.downloadLabel }</span>
        </Button>
        : null }
    </div>

    <Button
      className='mt-6 w-full'
      color='primary'
      isLoading={props.isBusy}
      onPress={props.onDone}
    >
      <span>{ props.actionLabel }</span>
    </Button>
  </div>
}
