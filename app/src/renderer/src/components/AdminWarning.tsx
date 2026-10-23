import { session } from '../lib/roomSession'

/** Aviso quando o app foi aberto como administrador (o Windows bloqueia a captura de tela). */
export default function AdminWarning(): React.JSX.Element | null {
  if (!session.elevated) return null
  return (
    <div className="banner warn">
      O app está rodando como administrador (por exemplo, aberto por um VS Code ou terminal em modo administrador).
      Nesse modo o Windows bloqueia a captura de tela: você consegue assistir, mas não transmitir. Feche e abra o app
      normalmente, sem administrador.
    </div>
  )
}
