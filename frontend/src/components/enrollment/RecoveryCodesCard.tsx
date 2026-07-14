// Copyright © 2026 Jalapeno Labs

// UI
import { Button } from '@heroui/react'

type Props = {
  codes: string[]
  title: string
  subtitle: string
  copyLabel: string
  actionLabel: string
  onContinue: () => void
  isBusy?: boolean
}

// The one-time recovery-codes display, shown after any enrollment that issues
// them (first-run claim, invite acceptance). The codes are shown exactly once, so
// this offers a copy-all and a deliberate "I've saved them" acknowledgement before
// moving on. Presentational: copy is passed in so each flow can word it. See
// docs/auth.md.
export function RecoveryCodesCard(props: Props) {
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
    <Button
      className='mt-3 w-full'
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
      className='mt-6 w-full'
      color='primary'
      isLoading={props.isBusy}
      onPress={props.onContinue}
    >
      <span>{ props.actionLabel }</span>
    </Button>
  </>
}
