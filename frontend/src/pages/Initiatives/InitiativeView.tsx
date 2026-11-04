// Copyright © 2026 Jalapeno Labs

import type { Initiative } from '../../api/routes/initiativeRoutes'

// Core
import { useState } from 'react'
import { useWatch } from 'react-hook-form'
import { useTranslation } from 'react-i18next'

// Redux
import { useAppSelector } from '../../store/hooks'
import { selectInitiativeHistory } from '../../store/actionItemHistorySlice'

// User interface
import { Alert, Chip } from '@heroui/react'
import { LuTrash2 } from 'react-icons/lu'
import { InlineEditableText } from '../../components/InlineEditableText'
import { Page } from '../../layout/page/Page'
import { HistoryTimeline } from '../ActionItems/HistoryTimeline'
import { InitiativeContainers } from './InitiativeContainers'
import { InitiativeDetailsPanel } from './InitiativeDetailsPanel'
import { InitiativeMembers } from './InitiativeMembers'
import { InitiativeProgressSection } from './InitiativeProgressSection'
import { RestoreInitiativeButton } from './RestoreInitiativeButton'

// Misc
import { useInitiativeHistoryLoader } from '../../hooks/useServerData'
import { UrlTree } from '../../urls'
import {
  INITIATIVE_DESCRIPTION_MAX_CHARACTERS,
  INITIATIVE_NAME_MAX_CHARACTERS,
  initiativeStateChipColors,
  initiativeStateLabelKeys,
} from './initiativePresentation'
import { useInitiativeActions } from './useInitiativeActions'
import { useInitiativeEditor } from './useInitiativeEditor'

type Props = {
  initiative: Initiative
}

// One initiative, edited in place and kept by the page's Save. Its name is the page's title,
// renamed where it is shown, with its state beside it. Its description, then its progress as
// resolved and total with the burnup beneath, its items, the Jira and GitHub containers it
// follows, and its history, follow; adding and removing items and containers act at once, as
// actions do. A deleted initiative is shown read-only, with Restore.
export function InitiativeView(props: Props) {
  const { t } = useTranslation([ 'initiatives', 'actionItems', 'common' ])
  const initiative = props.initiative
  const actions = useInitiativeActions()
  const editor = useInitiativeEditor(initiative)
  const [ name, description ] = useWatch({ control: editor.form.control, name: [ 'name', 'description' ]})
  const historyStatus = useInitiativeHistoryLoader(initiative.id)
  const history = useAppSelector((state) => selectInitiativeHistory(state, initiative.id))
  const isDeleted = Boolean(initiative.deletedAt)
  const [ isDeleting, setIsDeleting ] = useState(false)

  // At once, with Undo: the page leaves for the list, and Undo comes back to it.
  async function remove() {
    setIsDeleting(true)
    try {
      await actions.remove(initiative, UrlTree.initiatives)
    }
    finally {
      setIsDeleting(false)
    }
  }

  return <Page
    title={isDeleted
      ? initiative.name
      : name}
    rename={isDeleted
      ? undefined
      : {
        label: t('page.renameLabel'),
        maxLength: INITIATIVE_NAME_MAX_CHARACTERS,
        onChange: (value) => editor.form.setValue('name', value, { shouldDirty: true }),
      }}
    save={isDeleted
      ? undefined
      : { save: editor.save, isDirty: editor.form.formState.isDirty }}
    badges={<Chip variant='soft' color={initiativeStateChipColors[initiative.state]}>{
      t(initiativeStateLabelKeys[initiative.state])
    }</Chip>}
    parents={[
      { label: t('actionItems:title'), href: UrlTree.actionItems },
      { label: t('title'), href: UrlTree.initiatives },
    ]}
    actions={isDeleted
      ? undefined
      : [{
        id: 'delete',
        label: t('common:actions.delete'),
        icon: LuTrash2,
        isDanger: true,
        isPending: isDeleting,
        onAction: () => void remove(),
      }]}
  >
    {isDeleted && <Alert status='warning' className='relaxed'>
      <Alert.Indicator />
      <Alert.Content>
        <Alert.Title>{t('page.deletedTitle')}</Alert.Title>
        <Alert.Description>{t('page.deletedBody')}</Alert.Description>
      </Alert.Content>
      <RestoreInitiativeButton initiative={initiative} />
    </Alert>}

    <div className='grid grid-cols-1 items-start gap-8 lg:grid-cols-[minmax(0,1fr)_minmax(0,20rem)]'>
      <div>
        <section className='relaxed'>
          <h2 className='compact text-lg font-semibold'>{t('page.descriptionHeading')}</h2>
          {isDeleted
            ? <p className='text-sm whitespace-pre-line opacity-80'>{initiative.description}</p>
            : <InlineEditableText
              id='initiative-description'
              value={description}
              label={t('page.describeLabel')}
              placeholder={t('page.addDescription')}
              isMultiline
              maxLength={INITIATIVE_DESCRIPTION_MAX_CHARACTERS}
              className='text-sm whitespace-pre-line opacity-80'
              onChange={(value) => editor.form.setValue('description', value, { shouldDirty: true })}
            />}
        </section>
        <section className='relaxed'>
          <h2 className='compact text-lg font-semibold'>{t('progress.heading')}</h2>
          <InitiativeProgressSection initiative={initiative} />
        </section>
        <section className='relaxed'>
          <h2 className='compact text-lg font-semibold'>{t('members.heading')}</h2>
          <InitiativeMembers initiative={initiative} />
        </section>
        <InitiativeContainers initiative={initiative} />
        <section>
          <h2 className='compact text-lg font-semibold'>{t('actionItems:history.heading')}</h2>
          <HistoryTimeline entries={history} subject='initiative' status={historyStatus} />
        </section>
      </div>
      <aside className='lg:sticky lg:top-4'>
        <InitiativeDetailsPanel initiative={initiative} form={editor.form} />
      </aside>
    </div>
  </Page>
}
