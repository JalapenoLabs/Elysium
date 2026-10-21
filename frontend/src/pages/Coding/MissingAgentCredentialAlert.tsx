// Copyright © 2026 Jalapeno Labs

// Core
import { useTranslation } from 'react-i18next'

// Redux
import { useAppSelector } from '../../store/hooks'
import { selectAllLlms } from '../../store/llmsSlice'

// User interface
import { Alert, Link } from '@heroui/react'

// Misc
import { useLlmsLoader } from '../../hooks/useServerData'
import { UrlTree } from '../../urls'
import { hasAgentCredential } from '../Settings/Llms/llmPresentation'

// Warns before a session starts that its turns have no Claude credential to run on. Without
// one the CLI answers only "Not logged in", which says nothing about where to fix it.
export function MissingAgentCredentialAlert() {
  const { t } = useTranslation('coding')
  const status = useLlmsLoader()
  const llms = useAppSelector(selectAllLlms)

  // Until the list loads, nothing is known to be missing.
  if (status !== 'loaded' || hasAgentCredential(llms, Date.now())) {
    return null
  }

  return <Alert status='warning' className='compact mt-3'>
    <Alert.Indicator />
    <Alert.Content>
      <Alert.Title>{
        t('create.missingCredential.title')
      }</Alert.Title>
      <Alert.Description>{
        t('create.missingCredential.body')
      }</Alert.Description>
      <Link href={UrlTree.settingsLlms} className='text-link text-sm'>{
        t('create.missingCredential.link')
      }</Link>
    </Alert.Content>
  </Alert>
}
