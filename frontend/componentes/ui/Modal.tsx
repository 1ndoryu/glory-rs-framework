/* Glory-rs Framework: Componente Modal atomico.
 * Renderiza un overlay con contenido centrado. Cierra con click fuera o tecla Escape.
 * Dependencia: lucide-react (para icono X de cerrar). */

import { ReactNode, useEffect, useRef } from 'react';
import { X } from 'lucide-react';
import { Boton } from './index';
import '../../estilos/Componentes.css';

interface ModalProps {
  abierto: boolean;
  onCerrar: () => void;
  titulo: string;
  children: ReactNode;
}

function Modal({ abierto, onCerrar, titulo, children }: ModalProps) {
  const refOverlay = useRef<HTMLDivElement>(null);

  useEffect(() => {
    if (!abierto) return;

    const manejarEscape = (e: KeyboardEvent) => {
      if (e.key === 'Escape') onCerrar();
    };
    document.addEventListener('keydown', manejarEscape);
    document.body.style.overflow = 'hidden';

    return () => {
      document.removeEventListener('keydown', manejarEscape);
      document.body.style.overflow = '';
    };
  }, [abierto, onCerrar]);

  if (!abierto) return null;

  const manejarClickOverlay = (e: React.MouseEvent) => {
    if (e.target === refOverlay.current) onCerrar();
  };

  return (
    <div className="modalOverlay" ref={refOverlay} onClick={manejarClickOverlay}>
      <div className="modalContenido">
        <div className="modalCabecera">
          <h2 className="modalTitulo">{titulo}</h2>
          <Boton variante="fantasma" tamano="sm" onClick={onCerrar}>
            <X size={18} />
          </Boton>
        </div>
        <div className="modalCuerpo">
          {children}
        </div>
      </div>
    </div>
  );
}

export default Modal;
