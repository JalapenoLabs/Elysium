// Core
import { createBrowserRouter } from 'react-router'

// UI
import { RootLayout } from './components/RootLayout'
import { Home } from './pages/Home/Home'
import { About } from './pages/About/About'

// Misc
import { UrlTree } from './urls'

export const router = createBrowserRouter([
  {
    element: <RootLayout />,
    children: [
      {
        path: UrlTree.home,
        element: <Home />,
      },
      {
        path: UrlTree.about,
        element: <About />,
      },
    ],
  },
])
