// Copyright © 2026 Jalapeno Labs

// UI
import { Button } from '@heroui/react'

// The filename the codes download as. Not user-facing copy (a filename is an
// identifier), so it stays English regardless of locale.
const RECOVERY_CODES_FILENAME = 'elysium-recovery-codes.txt'

type Props = {
  codes: string[]
  title: string
  subtitle: string
  copyLabel: string
  downloadLabel: string
  actionLabel: string
  onContinue: () => void
  isBusy?: boolean
}

// The one-time recovery-codes display, shown after any enrollment that issues
// them (first-run claim, invite acceptance). The codes are shown exactly once, so
// this offers copy-all, download-as-file, and a deliberate "I've saved them"
// acknowledgement before moving on. Presentational: copy is passed in so each
// flow can word it. See docs/auth.md.
export function RecoveryCodesCard(props: Props) {
  // Saves the codes as a plain .txt via a transient object URL, revoked right
  // after the click so nothing lingers.
  function onDownload() {
    const blob = new Blob([ `${props.codes.join('\n')}\n` ], { type: 'text/plain' })
    const url = URL.createObjectURL(blob)
    const anchor = document.createElement('a')
    anchor.href = url
    anchor.download = RECOVERY_CODES_FILENAME
    anchor.click()
    URL.revokeObjectURL(url)
  }

  return <>
    <h1 className='text-2xl'>
      <strong>{ props.title }</strong>
    </h1>
    <p className='mt-3 opacity-80'>{ props.subtitle }</p>

    <div className='mt-6 grid grid-cols-2 gap-2 rounded-xl bg-content2 p-4 font-mono text-sm'>
      { props.codes.map((recoveryCode) =>
        <span key={recoveryCode}>{ recoveryCode }</span>,
      ) }
    </div>

    <div className='mt-3 level'>
      <Button
        className='w-full'
        variant='flat'
        onPress={() => {
          // Best-effort: clipboard is unavailable over plain HTTP / in some embeds.
          navigator.clipboard?.writeText(props.codes.join('\n')).catch((error) => {
            console.debug('clipboard write failed', { error })
          })
        }}
      >
        <span>{ props.copyLabel }</span>
      </Button>
      <Button
        className='w-full'
        variant='flat'
        onPress={onDownload}
      >
        <span>{ props.downloadLabel }</span>
      </Button>
    </div>

    <Button
      className='mt-6 w-full'
      color='primary'
      isLoading={props.isBusy}
      onPress={props.onContinue}
    >
      <span>{ props.actionLabel }</span>
    </Button>
  </>
}
