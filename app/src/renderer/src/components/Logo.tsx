import { Cast } from 'lucide-react'

/** Marca do app: um ícone de "transmitir" num selo com o gradiente do tema. */
export default function Logo({ size = 28 }: { size?: number }): React.JSX.Element {
  return (
    <span className="logo" style={{ width: size, height: size, borderRadius: Math.round(size * 0.28) }}>
      <Cast size={Math.round(size * 0.6)} strokeWidth={2.1} />
    </span>
  )
}
