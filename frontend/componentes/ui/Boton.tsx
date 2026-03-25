/* Glory-rs Framework: Componente Boton con variantes reales.
 * variante: primario/secundario/peligro/exito/fantasma
 * tamano: sm/md/lg
 * cargando: muestra spinner, ancho: width 100%, icono: ReactNode */

import { ButtonHTMLAttributes, ReactNode } from 'react';
import '../../estilos/Componentes.css';

type VarianteBoton = 'primario' | 'secundario' | 'peligro' | 'exito' | 'fantasma';
type TamanoBoton = 'sm' | 'md' | 'lg';

interface BotonProps extends ButtonHTMLAttributes<HTMLButtonElement> {
  variante?: VarianteBoton;
  tamano?: TamanoBoton;
  cargando?: boolean;
  ancho?: boolean;
  icono?: ReactNode;
}

const claseVariante: Record<VarianteBoton, string> = {
  primario: 'botonPrimario',
  secundario: 'botonSecundario',
  peligro: 'botonPeligro',
  exito: 'botonExito',
  fantasma: 'botonFantasma',
};

const claseTamano: Record<TamanoBoton, string> = {
  sm: 'botonSm',
  md: 'botonMd',
  lg: 'botonLg',
};

function Boton({
  variante = 'primario',
  tamano = 'md',
  cargando = false,
  ancho = false,
  icono,
  className,
  children,
  disabled,
  ...rest
}: BotonProps) {
  const clases = [
    'boton',
    claseVariante[variante],
    claseTamano[tamano],
    ancho ? 'botonAncho' : '',
    cargando ? 'botonCargando' : '',
    className ?? '',
  ].filter(Boolean).join(' ');

  return (
    <button className={clases} disabled={disabled || cargando} {...rest}>
      {icono && <span>{icono}</span>}
      {children}
    </button>
  );
}

export default Boton;
