/* Glory-rs Framework: Componente Select con variantes.
 * Sin etiqueta: renderiza solo el select estilizado (backward-compatible).
 * Con etiqueta: renderiza wrapper completo con label + error. */

import { SelectHTMLAttributes } from 'react';
import '../../estilos/Componentes.css';

type TamanoCampo = 'sm' | 'md' | 'lg';

interface SelectProps extends SelectHTMLAttributes<HTMLSelectElement> {
  tamano?: TamanoCampo;
  etiqueta?: string;
  error?: string;
}

const claseTamano: Record<TamanoCampo, string> = {
  sm: 'campoSm',
  md: 'campoMd',
  lg: 'campoLg',
};

function Select({ tamano = 'md', etiqueta, error, className, id, children, ...rest }: SelectProps) {
  const selectId = id ?? etiqueta?.toLowerCase().replace(/\s+/g, '-');
  const selectClases = `campoEntrada ${claseTamano[tamano]} ${className ?? ''}`.trim();

  if (!etiqueta && !error) {
    return <select id={selectId} className={selectClases} {...rest}>{children}</select>;
  }

  return (
    <div className={`campo ${error ? 'campoError' : ''}`}>
      {etiqueta && <label className="campoEtiqueta" htmlFor={selectId}>{etiqueta}</label>}
      <select id={selectId} className={selectClases} {...rest}>{children}</select>
      {error && <span className="campoMensajeError">{error}</span>}
    </div>
  );
}

export default Select;
