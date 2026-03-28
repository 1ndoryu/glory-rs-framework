# Glory-rs Framework

Framework interno reutilizable con componentes UI atómicos (React) y módulos backend fundacionales (Rust/Axum).

## Backend (Rust)

Crate `glory-backend` en `backend/` con módulos genéricos para web apps:

| Módulo | Descripción |
|--------|-------------|
| `errors` | AppError enum → HTTP status + JSON response |
| `config` | AppConfig + SmtpConfig desde variables de entorno (incluye error_report_email opcional) |

### Uso como dependencia

```toml
[dependencies]
glory-backend = { path = "glory-rs/backend" }
```

## Frontend (React + TypeScript)

### Componentes UI

| Componente | Descripcion |
|------------|-------------|
| `Boton` | Boton con variantes (primario/secundario/peligro/exito/fantasma), tamanos (sm/md/lg), loading, full-width |
| `Input` | Campo de texto con etiqueta opcional, validacion de error, tamanos |
| `Select` | Selector con etiqueta opcional, validacion de error, tamanos |
| `Textarea` | Area de texto con etiqueta opcional, validacion de error, tamanos |
| `Modal` | Overlay modal con cierre por Escape/click-fuera, animaciones, responsive bottom-sheet en mobile |

## Uso como submodulo

```bash
git submodule add <url-del-repo> glory-rs
```

En `vite.config.ts`:
```typescript
resolve: {
  alias: {
    '@glory': path.resolve(__dirname, '../glory-rs/frontend'),
  },
},
```

En `tsconfig.json`:
```json
{
  "compilerOptions": {
    "baseUrl": ".",
    "paths": {
      "@glory/*": ["../glory-rs/frontend/*"]
    }
  }
}
```

Import:
```typescript
import { Boton, Input, Modal } from '@glory/componentes/ui';
```

## Peer Dependencies

- `react` >= 18
- `lucide-react` >= 0.400 (solo si se usa Modal)
