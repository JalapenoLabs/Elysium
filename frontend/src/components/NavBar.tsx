// Copyright © 2026 Jalapeno Labs

// UI
import { Navbar, NavbarBrand, NavbarContent, NavbarItem, Link, Switch, Button } from '@heroui/react'

// Redux
import { useAppDispatch, useAppSelector } from '../store/hooks'
import { toggleTheme } from '../store/themeSlice'

// Misc
import { useAuth } from '../auth/useAuth'
import { UrlTree } from '../urls'

export function NavBar() {
  const dispatch = useAppDispatch()
  const themeMode = useAppSelector((state) => state.theme.mode)
  const auth = useAuth()

  return <Navbar maxWidth='lg' isBordered>
    <NavbarBrand>
      <span className='font-bold text-inherit'>Elysia</span>
    </NavbarBrand>
    <NavbarContent justify='center'>
      <NavbarItem>
        <Link color='foreground' href={UrlTree.home}>Home</Link>
      </NavbarItem>
      <NavbarItem>
        <Link color='foreground' href={UrlTree.about}>About</Link>
      </NavbarItem>
    </NavbarContent>
    <NavbarContent justify='end'>
      <NavbarItem>
        <Switch
          isSelected={themeMode === 'dark'}
          onValueChange={() => dispatch(toggleTheme())}
        >
          Dark
        </Switch>
      </NavbarItem>
      <NavbarItem>{
        auth.authenticated
          ? <Button variant='flat' onPress={auth.logout}>
            <span>Sign out{ auth.username && ` (${auth.username})` }</span>
          </Button>
          : <Button color='primary' onPress={auth.login}>
            <span>Sign in</span>
          </Button>
      }</NavbarItem>
    </NavbarContent>
  </Navbar>
}
