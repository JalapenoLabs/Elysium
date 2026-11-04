// Copyright © 2026 Jalapeno Labs

// Core
import { useState } from 'react'
import { useTranslation } from 'react-i18next'
import useSWR from 'swr'

// Redux
import { selectIsAdmin, selectIsCompanyMember } from '../../../store/authSlice'
import { useAppSelector } from '../../../store/hooks'

// User interface
import { Chip, Label, Spinner, TextArea, TextField } from '@heroui/react'
import { CopyButton } from '../../../components/CopyButton'
import { EmptyNotice } from '../../../components/EmptyNotice'
import { HelpPopover } from '../../../components/HelpPopover'
import { SettingsPage } from '../SettingsPage'
import { AgentGuidanceForm } from './AgentGuidanceForm'
import { AgentGuidanceVersions } from './AgentGuidanceVersions'

// Misc
import { AGENT_GUIDANCE_KEY, getAgentGuidance } from '../../../api/routes/agentGuidanceRoutes'

// `/settings/agent-guidance`: what every connected agent reads as its session starts. Admins
// edit it and save it from the header as the next version; members read it, and everyone copies
// the CLAUDE.md block. Below it are the rules Elysium adds for every agent, the block, and every
// version. Read from SWR alone, since no other view shows it; `agentGuidance.changed`
// revalidates it.
export function AgentGuidancePage() {
  const { t } = useTranslation([ 'agentGuidance', 'common' ])
  const isCompanyMember = useAppSelector(selectIsCompanyMember)
  const isAdmin = useAppSelector(selectIsAdmin)
  const guidance = useSWR(isCompanyMember
    ? AGENT_GUIDANCE_KEY
    : null, getAgentGuidance)
  const [ hasUnsavedEdits, setHasUnsavedEdits ] = useState(false)

  if (!isCompanyMember) {
    return <SettingsPage title={t('title')}>
      <EmptyNotice>{t('membersOnly')}</EmptyNotice>
    </SettingsPage>
  }

  const data = guidance.data

  return <SettingsPage
    title={t('title')}
    help={t('page.help')}
    badges={data && <Chip size='sm' variant='soft'>{
      t('page.version', { version: data.current.version })
    }</Chip>}
  >
    {data && <div className='flex max-w-3xl flex-col gap-8'>
      <section className='flex flex-col gap-4'>
        {/* The form registers the header's Save, for admins alone. */}
        {isAdmin
          ? <AgentGuidanceForm guidance={data} onDirtyChange={setHasUnsavedEdits} />
          : <TextField id='agent-guidance-content' className='w-full' isReadOnly value={data.current.content}>
            <Label>{t('form.label')}</Label>
            <TextArea rows={14} />
          </TextField>}
        <div>
          <p className='mb-1 text-xs font-medium opacity-70'>{t('rules.title')}</p>
          <p className='rounded-lg bg-surface-secondary px-3 py-2 text-sm'>{data.rules}</p>
        </div>
      </section>

      <section>
        <div className='compact flex items-center gap-1'>
          <h2 className='text-base font-semibold'>{t('claudeMd.title')}</h2>
          <HelpPopover>{t('claudeMd.help')}</HelpPopover>
        </div>
        <div className='flex items-start gap-2 rounded-lg bg-surface-secondary px-3 py-2'>
          <pre className='min-w-0 flex-1 overflow-x-auto whitespace-pre-wrap text-xs'>{data.claudeMd}</pre>
          <CopyButton value={data.claudeMd} label={t('claudeMd.copy')} />
        </div>
      </section>

      <section>
        <h2 className='compact text-base font-semibold'>{t('versions.title')}</h2>
        <AgentGuidanceVersions guidance={data} canRestore={isAdmin} hasUnsavedEdits={hasUnsavedEdits} />
      </section>
    </div>}
    {!data && guidance.error && <p className='py-10 text-center text-sm text-danger'>{t('loadError')}</p>}
    {!data && !guidance.error && <div className='grid place-items-center py-16'>
      <Spinner />
    </div>}
  </SettingsPage>
}
