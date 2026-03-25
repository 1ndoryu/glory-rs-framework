/* Glory-rs Framework: Componente Textarea con variantes.
 * Sin etiqueta: renderiza solo el textarea estilizado (backward-compatible).
 * Con etiqueta: renderiza wrapper completo con label + error. */

import { TextareaHTMLAttributes } from 'react';
import '../../estilos/Componentes.css';

type TamanoCampo = 'sm' | 'md' | 'lg';

interface TextareaProps extends TextareaHTMLAttributes<HTMLTextAreaElement> {
  tamano?: TamanoCampo;
  etiqueta?: string;
  error?: string;
}

const claseTamano: Record<TamanoCampo, string> = {
  sm: 'campoSm',
  md: 'campoMd',
  lg: 'campoLg',
};

function Textarea({ tamano = 'md', etiqueta, error, className, id, ...rest }: TextareaProps) {
  const textareaId = id ?? etiqueta?.toLowerCase().replace(/\s+/g, '-');
  const textareaClases = `campoEntrada ${claseTamano[tamano]} ${className ?? ''}`.trim();

  if (!etiqueta && !error) {
    return <textarea id={textareaId} className={textareaClases} {...rest} />;
  }

  return (
    <div className={`campo ${error ? 'campoError' : ''}`}>
      {etiqueta && <label className="campoEtiqueta" htmlFor={textareaId}>{etiqueta}</label>}
      <textarea id={textareaId} className={textareaClases} {...rest} />
      {error && <span className="campoMensajeError">{error}</span>}
    </div>
  );
}

export default Textarea;
