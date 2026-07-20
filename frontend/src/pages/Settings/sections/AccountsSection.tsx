// Copyright © 2026 Jalapeno Labs

// Core
import { useTranslation } from 'react-i18next'

// Redux
import { useAppSelector } from '../../../store/hooks'

// UI
import { SettingsSection } from '../../../components/settings/SettingsSection'

// The Accounts section: the organizations this account belongs to, with the
// caller's role in each. Read straight from the seeded session context.
export function AccountsSection() {
  const { t } = useTranslation([ 'auth', 'common' ])
  const orgs = useAppSelector((state) => state.sessionContext.orgs)

  return <SettingsSection
    title={t('auth:settings.accounts.title')}
    description={t('auth:settings.accounts.subtitle')}
  >
    { orgs.length
      ? <table className='w-full text-sm'>
        <thead>
          <tr className='border-b border-divider text-left text-xs uppercase tracking-wide text-default-500'>
            <th className='pb-2 font-medium'>{ t('auth:settings.accounts.colName') }</th>
            <th className='pb-2 font-medium'>{ t('auth:settings.accounts.colRole') }</th>
          </tr>
        </thead>
        <tbody className='divide-y divide-divider'>
          { orgs.map((org) =>
            <tr key={org.id}>
              <td className='py-3'>{ org.name }</td>
              <td className='py-3 text-default-500'>
                { t(`auth:settings.members.roles.${org.role}`, org.role) }
              </td>
            </tr>,
          ) }
        </tbody>
      </table>
      : <p className='text-sm text-default-500'>{ t('auth:settings.accounts.empty') }</p>
    }
  </SettingsSection>
}
