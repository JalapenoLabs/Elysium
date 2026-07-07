// UI
import { Navbar, NavbarBrand, NavbarContent, NavbarItem, Link, Switch } from '@heroui/react'

// Redux
import { useAppDispatch, useAppSelector } from '../store/hooks'
import { toggleTheme } from '../store/themeSlice'

// Misc
import { UrlTree } from '../urls'

export function NavBar() {
  const dispatch = useAppDispatch()
  const themeMode = useAppSelector((state) => state.theme.mode)

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
    </NavbarContent>
  </Navbar>
}
