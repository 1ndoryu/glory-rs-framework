/* Glory-rs Framework: adapter de DOM.
 * Concentra el acceso a document.body fuera de los componentes: sentinel
 * (dom-access-outside-platform) exige que el DOM se toque desde un boundary de
 * plataforma, y el nombre dom.ts es uno de sus defaults. */

/** Bloquea el scroll del documento y devuelve la función que lo restaura. */
export function bloquearScrollDocumento(): () => void {
  document.body.style.overflow = 'hidden';
  return () => {
    document.body.style.overflow = '';
  };
}
