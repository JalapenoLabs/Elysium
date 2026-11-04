// Copyright © 2026 Jalapeno Labs

// Core
import { useState } from 'react'
import { useTranslation } from 'react-i18next'
import useSWR from 'swr'

// User interface
import { Alert, Button, Form, Spinner } from '@heroui/react'
import { OptionSelect } from '../../../components/OptionSelect'

// Misc
import { getApiErrorMessage } from '../../../api/errors'
import {
  chooseJiraDoneTransition,
  forgetJiraDoneTransition,
  getJiraDoneTransition,
} from '../../../api/routes/jiraRoutes'
import { usePageSave } from '../../../layout/page/pageSave'
import { toast } from '../../../toast/toast'

type Props = {
  credentialId: string
  projectKey: string
}

// One project's done status: the one chosen, the only one, or a choice still to make. A
// choice is saved from the page's Save; Forget acts at once. The project's done statuses are
// read from Jira each time, so a status added there shows here.
export function JiraDoneTransitionPanel(props: Props) {
  const { t } = useTranslation([ 'jira', 'common' ])
  // Null until a status is picked, so the stored choice shows until then.
  const [ picked, setPicked ] = useState<string | null>(null)
  const [ isForgetting, setIsForgetting ] = useState(false)
  const reading = useSWR(
    [ 'jira-done-transition', props.credentialId, props.projectKey ],
    ([ , credentialId, projectKey ]) => getJiraDoneTransition(credentialId, projectKey),
    { revalidateOnFocus: false, shouldRetryOnError: false },
  )
  const transition = reading.data
  const stored = transition?.chosen?.statusId ?? ''
  const shown = picked ?? stored

  async function save() {
    if (!transition) {
      console.debug('JiraDoneTransitionPanel was saved before the project was read')
      return false
    }
    try {
      const response = await chooseJiraDoneTransition(props.credentialId, props.projectKey, shown)
      await reading.mutate({ ...transition, chosen: response.chosen }, { revalidate: false })
      setPicked(null)
      toast.success(t('doneTransitions.chosen', { project: props.projectKey, status: response.chosen.statusName }))
      return true
    }
    catch (error) {
      console.debug('JiraDoneTransitionPanel failed to choose a done status', { error, statusId: shown })
      toast.danger(getApiErrorMessage(error) ?? t('common:errors.unexpected'))
      return false
    }
  }

  const onSubmit = usePageSave({
    save,
    isDirty: Boolean(shown) && shown !== stored,
  })

  async function forget() {
    setIsForgetting(true)
    try {
      await forgetJiraDoneTransition(props.credentialId, props.projectKey)
      // Whether a choice is now needed depends on Jira's statuses, so the answer is read again.
      await reading.mutate()
      setPicked(null)
      toast.success(t('doneTransitions.forgotten', { project: props.projectKey }))
    }
    catch (error) {
      console.debug('JiraDoneTransitionPanel failed to forget a done status', { error })
      toast.danger(getApiErrorMessage(error) ?? t('common:errors.unexpected'))
    }
    finally {
      setIsForgetting(false)
    }
  }

  if (reading.error) {
    return <p className='text-sm text-danger'>{
      getApiErrorMessage(reading.error) ?? t('common:errors.unexpected')
    }</p>
  }

  if (!transition) {
    return <div className='grid max-w-md place-items-center py-6'>
      <Spinner size='sm' />
    </div>
  }

  if (!transition.statuses.length && !transition.chosen) {
    return <Alert status='warning' className='max-w-2xl'>
      <Alert.Indicator />
      <Alert.Content>
        <Alert.Title>{t('doneTransitions.noneTitle')}</Alert.Title>
        <Alert.Description>{t('doneTransitions.noneBody', { project: props.projectKey })}</Alert.Description>
      </Alert.Content>
    </Alert>
  }

  const statusOptions = transition.statuses.map((status) => ({ id: status.id, label: status.name }))
  const onlyStatus = transition.statuses.length === 1
    ? transition.statuses[0]
    : null

  return <Form onSubmit={onSubmit} className='flex max-w-md flex-col gap-4'>
    {/* `needsChoice` says the project has several done statuses, chosen or not. */}
    {transition.needsChoice && !transition.chosen && <Alert status='warning'>
      <Alert.Indicator />
      <Alert.Content>
        <Alert.Title>{t('doneTransitions.needsChoiceTitle')}</Alert.Title>
        <Alert.Description>{t('doneTransitions.needsChoiceBody')}</Alert.Description>
      </Alert.Content>
    </Alert>}

    {!transition.chosen && onlyStatus && <p className='text-sm'>{
      t('doneTransitions.automatic', { status: onlyStatus.name })
    }</p>}

    {transition.chosen && <p className='text-sm'>{
      t('doneTransitions.current', { status: transition.chosen.statusName })
    }</p>}

    {statusOptions.length > 1 && <OptionSelect
      id='jira-done-transition-status'
      label={t('doneTransitions.status')}
      options={statusOptions}
      value={shown}
      onChange={setPicked}
    />}

    {transition.chosen && <div>
      <Button size='sm' variant='tertiary' isPending={isForgetting} onPress={() => void forget()}>
        <span>{t('doneTransitions.forget')}</span>
      </Button>
    </div>}
  </Form>
}
