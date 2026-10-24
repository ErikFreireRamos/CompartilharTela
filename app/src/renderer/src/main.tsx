import { lazy, Suspense } from 'react'
import { createRoot } from 'react-dom/client'
import App from './App'
import './styles.css'

// A janela "destacada" carrega o mesmo bundle, mas só o visualizador.
const Popout = lazy(() => import('./pages/Popout'))
const isPopout = new URLSearchParams(location.search).has('popout')

createRoot(document.getElementById('root')!).render(
  isPopout ? (
    <Suspense fallback={null}>
      <Popout />
    </Suspense>
  ) : (
    <App />
  )
)
