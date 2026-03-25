/* Glory-rs Framework: Componente Input con variantes.
 * Sin etiqueta: renderiza solo el input estilizado (backward-compatible).
 * Con etiqueta: renderiza wrapper completo con label + error. */

import { InputHTMLAttributes } from 'react';
import '../../estilos/Componentes.css';

type TamanoCampo = 'sm' | 'md' | 'lg';

interface InputProps extends InputHTMLAttributes<HTMLInputElement> {
  tamano?: TamanoCampo;
  etiqueta?: string;
  error?: string;
}

const claseTamano: Record<TamanoCampo, string> = {
  sm: 'campoSm',
  md: 'campoMd',
  lg: 'campoLg',
};

function Input({ tamano = 'md', etiqueta, error, className, id, ...rest }: InputProps) {
  const inputId = id ?? etiqueta?.toLowerCase().replace(/\s+/g, '-');
  const inputClases = `campoEntrada ${claseTamano[tamano]} ${className ?? ''}`.trim();

  if (!etiqueta && !error) {
    return <input id={inputId} className={inputClases} {...rest} />;
  }

  return (
    <div className={`campo ${error ? 'campoError' : ''}`}>
      {etiqueta && <label className="campoEtiqueta" htmlFor={inputId}>{etiqueta}</label>}
      <input id={inputId} className={inputClases} {...rest} />
      {error && <span className="campoMensajeError">{error}</span>}
    </div>
  );
}

export default Input;
