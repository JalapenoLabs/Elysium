// Copyright © 2026 Jalapeno Labs

import type { ProjectAutonomy } from '../../api/routes/projectRoutes'

// Core
import { useTranslation } from 'react-i18next'
import useSWR from 'swr'

// User interface
import { Radio, RadioGroup } from '@heroui/react'
import { HelpPopover } from '../../components/HelpPopover'

// Misc
import { getProjectAutonomy, getProjectAutonomyKey, PROJECT_AUTONOMY_LEVELS } from '../../api/routes/projectRoutes'
import { formatDateTime } from '../../format/dates'
import { useUserNames } from '../../hooks/useUserNames'
import { autonomyDescriptionKeys, autonomyLabelKeys } from './projectAutonomyPresentation'

type Props = {
  projectId: string
  // The level as the page's form holds it, which its Save sends.
  value: ProjectAutonomy
  onChange: (autonomy: ProjectAutonomy) => void
  // Only the company's members turn the dial; a guest reads it.
  isEditable: boolean
}

// The project's trust dial, a field of the project page kept by its Save: what of its Project
// Manager's work waits for the person. Who set the saved level and when is read from SWR alone,
// since no other view shows it, and `project.upserted` revalidates it.
export function ProjectAutonomyField(props: Props) {
  const { t } = useTranslation([ 'projects', 'common' ])
  const userNames = useUserNames()
  const setting = useSWR(
    getProjectAutonomyKey(props.projectId),
    () => getProjectAutonomy(props.projectId),
  ).data?.autonomy

  let provenance: string | null = null
  if (setting?.setBy && setting.setAt) {
    provenance = t('autonomy.setBy', {
      name: userNames[setting.setBy] ?? t('common:table.someone'),
      date: formatDateTime(setting.setAt),
    })
  }
  else if (setting) {
    provenance = t('autonomy.startedHere')
  }

  return <section className='relaxed max-w-4xl'>
    <div className='compact flex items-center gap-1'>
      <h2 className='text-xl font-semibold'>{t('autonomy.heading')}</h2>
      <HelpPopover label={t('autonomy.helpLabel')}>
        <p>{t('autonomy.help')}</p>
      </HelpPopover>
    </div>
    <RadioGroup
      id='project-autonomy'
      aria-label={t('autonomy.label')}
      value={props.value}
      isDisabled={!props.isEditable}
      onChange={(value) => {
        const level = PROJECT_AUTONOMY_LEVELS.find((option) => option === value)
        if (!level) {
          console.debug('ProjectAutonomyField ignored an unknown level', { value })
          return
        }
        props.onChange(level)
      }}
      className='compact grid grid-cols-1 gap-3 sm:grid-cols-3'
    >{
      PROJECT_AUTONOMY_LEVELS.map((level) => <Radio key={level} value={level} className='m-0'>
        <Radio.Content
          className={[
            'flex h-full w-full cursor-pointer flex-col gap-1 rounded-xl border-2 border-separator p-3',
            'transition-colors data-[hovered=true]:border-accent/40',
            'data-[selected=true]:border-accent data-[selected=true]:ring-4 data-[selected=true]:ring-accent/15',
            'data-[focus-visible=true]:ring-4 data-[focus-visible=true]:ring-accent/30',
            'data-[disabled=true]:cursor-default',
          ].join(' ')}
        >
          <div className='flex items-center gap-2'>
            <Radio.Control className='border border-separator'>
              <Radio.Indicator />
            </Radio.Control>
            <span className='text-sm font-semibold'>{t(autonomyLabelKeys[level])}</span>
          </div>
          <p className='text-sm opacity-70'>{t(autonomyDescriptionKeys[level])}</p>
        </Radio.Content>
      </Radio>)
    }</RadioGroup>
    <p className='text-xs opacity-60'>{
      props.isEditable
        ? provenance
        : t('autonomy.membersOnly')
    }</p>
  </section>
}
