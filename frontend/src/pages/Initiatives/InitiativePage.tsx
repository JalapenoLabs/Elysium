// Copyright © 2026 Jalapeno Labs

// Core
import { useTranslation } from 'react-i18next'
import { useNavigate, useParams } from 'react-router'

// Redux
import { useAppDispatch, useAppSelector } from '../../store/hooks'
import { selectInitiativeHistory } from '../../store/actionItemHistorySlice'
import { initiativeUpserted, selectInitiativeById } from '../../store/initiativesSlice'

// User interface
import { Alert, Breadcrumbs, Button, Chip, Link, Spinner, toast } from '@heroui/react'
import { LuTrash2 } from 'react-icons/lu'
import { InlineEditableText } from '../../components/InlineEditableText'
import { HistoryTimeline } from '../ActionItems/HistoryTimeline'
import { InitiativeDetailsPanel } from './InitiativeDetailsPanel'
import { InitiativeMembers } from './InitiativeMembers'
import { InitiativeProgressSection } from './InitiativeProgressSection'
import { RestoreInitiativeButton } from './RestoreInitiativeButton'

// Misc
import { updateInitiative } from '../../api/routes/initiativeRoutes'
import { useInitiativeHistoryLoader, useInitiativeLoader } from '../../hooks/useServerData'
import { UrlTree } from '../../urls'
import {
  INITIATIVE_DESCRIPTION_MAX_CHARACTERS,
  INITIATIVE_NAME_MAX_CHARACTERS,
  initiativeStateChipColors,
  initiativeStateLabelKeys,
} from './initiativePresentation'
import { useInitiativeActions } from './useInitiativeActions'

// `/action-items/initiatives/:initiativeId`: one initiative, edited in place. Progress leads,
// as resolved and total with the burnup beneath; its items, and its history, follow. A
// deleted initiative is shown read-only, with Restore.
export function InitiativePage() {
  const { t } = useTranslation([ 'initiatives', 'actionItems', 'common' ])
  const dispatch = useAppDispatch()
  const navigate = useNavigate()
  const actions = useInitiativeActions()
  const { initiativeId = '' } = useParams()
  const status = useInitiativeLoader(initiativeId)
  const historyStatus = useInitiativeHistoryLoader(initiativeId)
  const initiative = useAppSelector((state) => selectInitiativeById(state, initiativeId))
  const history = useAppSelector((state) => selectInitiativeHistory(state, initiativeId))

  if (!initiative) {
    return <div className='container'>{
      status === 'loading'
        ? <div className='grid place-items-center py-16'>
          <Spinner />
        </div>
        : <div className='py-16 text-center'>
          <p className='compact text-sm opacity-70'>{t('page.notFound')}</p>
          <Link href={UrlTree.initiatives} className='text-link'>{t('page.backToList')}</Link>
        </div>
    }</div>
  }

  const isDeleted = Boolean(initiative.deletedAt)

  // Throws on failure, so the field stays open on what was typed.
  async function saveText(changes: { name: string } | { description: string }) {
    if (!initiative) {
      console.debug('InitiativePage saved a field with no initiative loaded')
      return
    }
    try {
      const response = await updateInitiative(initiative.id, changes)
      dispatch(initiativeUpserted(response.initiative))
    }
    catch (error) {
      console.debug('InitiativePage failed to save the initiative', { error, initiativeId: initiative.id })
      toast.danger(t('common:errors.unexpected'))
      throw error
    }
  }

  return <div className='container'>
    <Breadcrumbs className='compact'>
      <Breadcrumbs.Item href={UrlTree.actionItems}>{t('actionItems:title')}</Breadcrumbs.Item>
      <Breadcrumbs.Item href={UrlTree.initiatives}>{t('title')}</Breadcrumbs.Item>
      <Breadcrumbs.Item>{initiative.name}</Breadcrumbs.Item>
    </Breadcrumbs>

    {isDeleted && <Alert status='warning' className='relaxed'>
      <Alert.Indicator />
      <Alert.Content>
        <Alert.Title>{t('page.deletedTitle')}</Alert.Title>
        <Alert.Description>{t('page.deletedBody')}</Alert.Description>
      </Alert.Content>
      <RestoreInitiativeButton initiative={initiative} />
    </Alert>}

    <div className='level relaxed items-start gap-4'>
      <div className='min-w-0 flex-1'>
        {isDeleted
          ? <h1 className='text-3xl font-bold'>{initiative.name}</h1>
          : <InlineEditableText
            value={initiative.name}
            label={t('page.renameLabel')}
            isRequired
            maxLength={INITIATIVE_NAME_MAX_CHARACTERS}
            className='text-3xl font-bold'
            onSave={(name) => saveText({ name })}
          />}
        <div className='mt-1 max-w-3xl'>
          {isDeleted
            ? <p className='text-sm whitespace-pre-line opacity-80'>{initiative.description}</p>
            : <InlineEditableText
              value={initiative.description}
              label={t('page.describeLabel')}
              placeholder={t('page.addDescription')}
              isMultiline
              maxLength={INITIATIVE_DESCRIPTION_MAX_CHARACTERS}
              className='text-sm whitespace-pre-line opacity-80'
              onSave={(description) => saveText({ description })}
            />}
        </div>
      </div>
      <div className='flex shrink-0 items-center gap-2'>
        <Chip variant='soft' color={initiativeStateChipColors[initiative.state]}>{
          t(initiativeStateLabelKeys[initiative.state])
        }</Chip>
        {!isDeleted && <Button
          size='sm'
          variant='ghost'
          onPress={() => actions.remove(initiative, () => navigate(UrlTree.initiatives))}
        >
          <LuTrash2 className='size-4' aria-hidden />
          <span>{t('common:actions.delete')}</span>
        </Button>}
      </div>
    </div>

    <div className='grid grid-cols-1 items-start gap-8 lg:grid-cols-[minmax(0,1fr)_minmax(0,20rem)]'>
      <div>
        <section className='relaxed'>
          <h2 className='compact text-lg font-semibold'>{t('progress.heading')}</h2>
          <InitiativeProgressSection initiative={initiative} />
        </section>
        <section className='relaxed'>
          <h2 className='compact text-lg font-semibold'>{t('members.heading')}</h2>
          <InitiativeMembers initiative={initiative} />
        </section>
        <section>
          <h2 className='compact text-lg font-semibold'>{t('actionItems:history.heading')}</h2>
          <HistoryTimeline entries={history} subject='initiative' status={historyStatus} />
        </section>
      </div>
      <aside className='lg:sticky lg:top-4'>
        <InitiativeDetailsPanel initiative={initiative} />
      </aside>
    </div>
  </div>
}
