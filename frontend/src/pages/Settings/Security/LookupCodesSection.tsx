// Copyright © 2026 Jalapeno Labs

import type { SettingsFlow } from '@ory/client-fetch'
import type { SettingsSubmission } from '../../../hooks/useKratosSettingsFlow'

// Core
import { useState } from 'react'
import { useTranslation } from 'react-i18next'

// Redux
import { selectMe } from '../../../store/authSlice'
import { useAppSelector } from '../../../store/hooks'

// User interface
import { Alert, Button } from '@heroui/react'
import { BackupCodesList } from './BackupCodesList'

// Misc
import { useConfirm } from '../../../hooks/useConfirm'
import { readLookupCodes } from './securityPresentation'

type Props = {
  flow: SettingsFlow
  onSubmit: (body: SettingsSubmission, savedMessage: string) => Promise<boolean>
}

type LookupAction = 'reveal' | 'regenerate' | 'confirm'

// What each button submits to Kratos.
const lookupBodies = {
  reveal: { method: 'lookup_secret', lookup_secret_reveal: true },
  regenerate: { method: 'lookup_secret', lookup_secret_regenerate: true },
  confirm: { method: 'lookup_secret', lookup_secret_confirm: true },
} as const satisfies Record<LookupAction, SettingsSubmission>

// One-time codes that stand in for the authenticator app when the phone is lost, on the Backup
// codes tab. Each works once; new ones replace the old only after this person confirms they
// saved them, which copying or downloading them helps with.
export function LookupCodesSection(props: Props) {
  const { t } = useTranslation([ 'auth', 'common' ])
  const confirm = useConfirm()
  const me = useAppSelector(selectMe)
  const [ pending, setPending ] = useState<LookupAction | null>(null)
  const lookup = readLookupCodes(props.flow)

  async function run(action: LookupAction, savedMessage: string) {
    setPending(action)
    await props.onSubmit(lookupBodies[action], savedMessage)
    setPending(null)
  }

  function disable() {
    confirm({
      title: t('security.lookup.disableTitle'),
      message: t('security.lookup.disableMessage'),
      isIrreversible: true,
      confirmText: t('security.lookup.disable'),
      onConfirm: async () => {
        await props.onSubmit({ method: 'lookup_secret', lookup_secret_disable: true }, t('security.lookup.disabled'))
      },
    })
  }

  return <section>
    {lookup.codes && <div className='relaxed'>
      {lookup.canConfirm && <Alert status='warning' className='compact'>
        <Alert.Indicator />
        <Alert.Content>
          <Alert.Description>{t('security.lookup.saveThem')}</Alert.Description>
        </Alert.Content>
      </Alert>}
      <BackupCodesList codes={lookup.codes} appName={me?.authenticatorNickname ?? ''} />
    </div>}

    <div className='level-left flex-wrap'>
      {lookup.canConfirm && <Button
        size='sm'
        isPending={pending === 'confirm'}
        onPress={() => run('confirm', t('security.lookup.confirmed'))}
      >
        <span>{t('security.lookup.confirm')}</span>
      </Button>}
      {lookup.canReveal && !lookup.codes && <Button
        size='sm'
        variant='outline'
        isPending={pending === 'reveal'}
        onPress={() => run('reveal', t('security.lookup.revealed'))}
      >
        <span>{t('security.lookup.reveal')}</span>
      </Button>}
      {lookup.canRegenerate && <Button
        size='sm'
        variant='outline'
        isPending={pending === 'regenerate'}
        onPress={() => run('regenerate', t('security.lookup.regenerated'))}
      >
        <span>{
          lookup.canReveal || lookup.canDisable
            ? t('security.lookup.regenerate')
            : t('security.lookup.generate')
        }</span>
      </Button>}
      {lookup.canDisable && <Button size='sm' variant='ghost' onPress={disable}>
        <span>{t('security.lookup.disable')}</span>
      </Button>}
    </div>
  </section>
}
