// Copyright © 2026 Jalapeno Labs

import type { AgentGuidance, AgentGuidanceVersion } from '../../../api/routes/agentGuidanceRoutes'

// Core
import { useState } from 'react'
import { useTranslation } from 'react-i18next'
import { useSWRConfig } from 'swr'

// User interface
import { Button, Chip, Tooltip } from '@heroui/react'
import { LuHistory } from 'react-icons/lu'
import { TextDiff } from '../../../components/TextDiff'
import { DateText } from '../../../format/DateText'

// Misc
import { getApiErrorMessage } from '../../../api/errors'
import { AGENT_GUIDANCE_KEY, restoreAgentGuidanceVersion } from '../../../api/routes/agentGuidanceRoutes'
import { useUserNames } from '../../../hooks/useUserNames'
import { toast } from '../../../toast/toast'

type Props = {
  guidance: AgentGuidance
  // Admins restore; everyone else reads.
  canRestore: boolean
  // While the guidance has unsaved edits, Restore waits for them to be saved or discarded.
  hasUnsavedEdits: boolean
}

// Every version, newest first: who wrote it and when, what it changed from the one before, and,
// for admins, Restore on any whose text is not current. Restoring acts at once, since it only
// adds a version: restoring the one before undoes it.
export function AgentGuidanceVersions(props: Props) {
  const { t } = useTranslation([ 'agentGuidance', 'common' ])
  const { mutate } = useSWRConfig()
  const userNames = useUserNames()
  const [ restoringVersion, setRestoringVersion ] = useState<number | null>(null)
  // One version's changes open at a time.
  const [ openVersion, setOpenVersion ] = useState<number | null>(null)
  const versions = props.guidance.versions
  const current = props.guidance.current

  async function restore(version: AgentGuidanceVersion) {
    setRestoringVersion(version.version)
    try {
      const response = await restoreAgentGuidanceVersion(version.version)
      void mutate(AGENT_GUIDANCE_KEY, response, { revalidate: false })
      toast.success(t('toasts.restored', { version: version.version }))
    }
    catch (error) {
      console.debug('AgentGuidanceVersions could not restore a version', { error, version: version.version })
      toast.danger(t('common:errors.unexpected'), { description: getApiErrorMessage(error) ?? undefined })
    }
    finally {
      setRestoringVersion(null)
    }
  }

  return <ol className='flex flex-col gap-4 border-l border-separator pl-4'>{
    versions.map((version, index) => {
      const who = userNames[version.createdBy] ?? t('common:table.someone')
      const sentence = version.restoredFrom === null
        ? t('versions.saved', { who, version: version.version })
        : t('versions.restored', { who, from: version.restoredFrom, version: version.version })
      const isOpen = openVersion === version.version
      const restoreLabel = t('versions.restore')

      return <li key={version.version} className='text-sm'>
        <div className='level items-start gap-4'>
          <div className='min-w-0'>
            <div className='flex flex-wrap items-center gap-2'>
              <p>{sentence}</p>
              {version.version === current.version && <Chip size='sm' variant='soft' color='accent'>{
                t('versions.current')
              }</Chip>}
            </div>
            <div className='mt-1 text-xs opacity-50'>
              <DateText value={version.createdAt} />
            </div>
            <Button
              size='sm'
              variant='ghost'
              className='-ml-2 mt-1'
              aria-expanded={isOpen}
              onPress={() => setOpenVersion(isOpen
                ? null
                : version.version)}
            >
              <span>{isOpen
                ? t('versions.hideChanges')
                : t('versions.showChanges')}</span>
            </Button>
          </div>
          {props.canRestore && version.content !== current.content && (props.hasUnsavedEdits
            // A disabled button fires no hover, so its tooltip hangs on a wrapper.
            ? <Tooltip delay={300}>
              <Tooltip.Trigger>
                <div>
                  <Button isIconOnly size='sm' variant='ghost' aria-label={restoreLabel} isDisabled>
                    <LuHistory className='size-4' aria-hidden />
                  </Button>
                </div>
              </Tooltip.Trigger>
              <Tooltip.Content>
                <span>{t('versions.saveFirst')}</span>
              </Tooltip.Content>
            </Tooltip>
            : <Tooltip delay={300}>
              <Button
                isIconOnly
                size='sm'
                variant='ghost'
                aria-label={restoreLabel}
                isPending={restoringVersion === version.version}
                isDisabled={restoringVersion !== null}
                onPress={() => restore(version)}
              >
                <LuHistory className='size-4' aria-hidden />
              </Button>
              <Tooltip.Content>
                <span>{restoreLabel}</span>
              </Tooltip.Content>
            </Tooltip>)}
        </div>
        {isOpen && <div className='mt-2'>
          <TextDiff
            before={versions[index + 1]?.content ?? ''}
            after={version.content}
            label={t('versions.changesLabel', { version: version.version })}
            foldedLabel={(count) => t('versions.foldedLines', { count })}
          />
        </div>}
      </li>
    })
  }</ol>
}
