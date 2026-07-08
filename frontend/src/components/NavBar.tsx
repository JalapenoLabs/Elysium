// Copyright © 2026 Jalapeno Labs

// Core
import { useNavigate } from 'react-router'
import { useTranslation } from 'react-i18next'

// UI
import { Navbar, NavbarBrand, NavbarContent, NavbarItem, Link, Switch, Button } from '@heroui/react'

// Redux
import { dispatch } from '../store'
import { useAppSelector } from '../store/hooks'
import { toggleTheme } from '../store/themeSlice'

// Misc
import { useSession } from '../auth/useSession'
import { UrlTree } from '../urls'

export function NavBar() {
  const { t } = useTranslation('common')
  const navigate = useNavigate()
  const themeMode = useAppSelector((state) => state.theme.mode)
  const session = useSession()

  return <Navbar maxWidth='lg' isBordered>
    <NavbarBrand>
      <span className='font-bold text-inherit'>{ t('aiName') }</span>
    </NavbarBrand>
    <NavbarContent justify='center'>
      <NavbarItem>
        <Link color='foreground' href={UrlTree.home}>{ t('nav.home') }</Link>
      </NavbarItem>
      <NavbarItem>
        <Link color='foreground' href={UrlTree.about}>{ t('nav.about') }</Link>
      </NavbarItem>
    </NavbarContent>
    <NavbarContent justify='end'>
      <NavbarItem>
        <Switch
          isSelected={themeMode === 'dark'}
          onValueChange={() => dispatch(toggleTheme())}
        >
          { t('theme.dark') }
        </Switch>
      </NavbarItem>
      <NavbarItem>{
        session.isAuthenticated
          ? <Button variant='flat' onPress={session.signOut}>
            <span>{
              session.principal?.username
                ? t('actions.signOutAs', { name: session.principal.username })
                : t('actions.signOut')
            }</span>
          </Button>
          : <Button color='primary' onPress={() => navigate(UrlTree.signIn)}>
            <span>{ t('actions.signIn') }</span>
          </Button>
      }</NavbarItem>
    </NavbarContent>
  </Navbar>
}
