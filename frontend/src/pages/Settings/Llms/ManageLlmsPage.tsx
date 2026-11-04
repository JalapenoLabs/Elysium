// Copyright © 2026 Jalapeno Labs

import type { Llm } from '../../../api/routes/llmRoutes'

// Core
import { useTranslation } from 'react-i18next'

// Redux
import { selectIsAdmin } from '../../../store/authSlice'
import { useAppDispatch, useAppSelector } from '../../../store/hooks'
import { llmDeleted, llmUpserted, selectAllLlms } from '../../../store/llmsSlice'

// User interface
import { Button, Dropdown, Label, Spinner } from '@heroui/react'
import { LuChevronDown, LuFolderGit2, LuPlus } from 'react-icons/lu'
import { SettingsPage } from '../SettingsPage'
import { LlmTable } from './LlmTable'

// Misc
import { deleteLlm, updateLlm } from '../../../api/routes/llmRoutes'
import { useConfirm } from '../../../hooks/useConfirm'
import { useLlmsLoader } from '../../../hooks/useServerData'
import { LLM_TYPES, llmTypeIcons, llmTypeLabelKeys } from './llmPresentation'
import { getNewLlmUrl, UrlTree } from '../../../urls'
import { llmSlugByType } from './llmProviders'
import { toast } from '../../../toast/toast'

// `/settings/llms`: every LLM credential, activated, deactivated, and deleted from its row, and
// added from the Add LLM menu, one page per provider. Admins also reach the agent
// configuration, a form of its own, from the page's actions.
export function ManageLlmsPage() {
  const { t } = useTranslation([ 'llms', 'common' ])
  const dispatch = useAppDispatch()
  const llms = useAppSelector(selectAllLlms)
  const status = useLlmsLoader()
  const isAdmin = useAppSelector(selectIsAdmin)
  const confirm = useConfirm()

  function confirmDelete(llm: Llm) {
    confirm({
      title: t('delete.title', { name: llm.name }),
      message: t('delete.body', { name: llm.name }),
      isIrreversible: true,
      confirmText: t('common:actions.delete'),
      onConfirm: async () => {
        try {
          await deleteLlm(llm.id)
        }
        catch (error) {
          console.debug('ManageLlmsPage failed to delete the LLM', { error, llmId: llm.id })
          toast.danger(t('common:errors.unexpected'))
          throw error
        }
        dispatch(llmDeleted(llm.id))
        toast.success(t('toasts.deleted', { name: llm.name }))
      },
    })
  }

  async function toggleActive(llm: Llm) {
    try {
      const response = await updateLlm(llm.id, { isActive: !llm.isActive })
      dispatch(llmUpserted(response.llm))
      toast.success(t('toasts.updated', { name: llm.name }))
    }
    catch (error) {
      console.debug('ManageLlmsPage failed to toggle an LLM', { error, llmId: llm.id })
      toast.danger(t('common:errors.unexpected'))
    }
  }

  return <SettingsPage
    title={t('title')}
    help={t('credentials.help')}
    primaryAction={<Dropdown>
      <Button variant='outline'>
        <LuPlus className='size-4' aria-hidden />
        <span>{t('credentials.add')}</span>
        <LuChevronDown className='size-4' aria-hidden />
      </Button>
      <Dropdown.Popover placement='bottom end'>
        {/* Each provider is a link to its own add page, so it opens in a new tab too. */}
        <Dropdown.Menu aria-label={t('credentials.addMenu')}>{
          LLM_TYPES.map((type) => {
            const Icon = llmTypeIcons[type]
            return <Dropdown.Item
              key={type}
              id={type}
              textValue={t(llmTypeLabelKeys[type])}
              href={getNewLlmUrl(llmSlugByType[type])}
            >
              <Icon className='size-4' aria-hidden />
              <Label>{t(llmTypeLabelKeys[type])}</Label>
            </Dropdown.Item>
          })
        }</Dropdown.Menu>
      </Dropdown.Popover>
    </Dropdown>}
    actions={isAdmin
      ? [{
        id: 'agentConfiguration',
        label: t('agentConfiguration.title'),
        icon: LuFolderGit2,
        href: UrlTree.settingsLlmsAgentConfiguration,
      }]
      : []}
  >
    {status === 'loading' && <div className='grid place-items-center py-16'>
      <Spinner />
    </div>}

    {status === 'failed' && <p className='py-10 text-center text-sm text-danger'>{
      t('table.loadError')
    }</p>}

    {status === 'loaded' && <LlmTable
      llms={llms}
      onToggleActive={toggleActive}
      onDelete={confirmDelete}
    />}
  </SettingsPage>
}
