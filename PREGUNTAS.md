# Preguntas para ti

Nada de esto bloqueó el trabajo: en cada caso elegí una opción y la anoté en `DECISIONES.md`.

1. **Notificaciones nativas de Windows.** Elegí dibujar una ventana de notificación propia (esquina inferior derecha, se cierra sola, desactivada por omisión) en lugar de las notificaciones del sistema. Motivo: los globos (`Shell_NotifyIconW`) hacen que el Explorador registre el ícono en `HKCU\Control Panel\NotifyIconSettings`, y las notificaciones *toast* necesitan registrar un AppUserModelID o crear un acceso directo en el menú Inicio, lo cual choca con «Vigía no escribe en el registro». ¿Prefieres las nativas (aparecen en el centro de notificaciones) aceptando esa escritura que hace Windows?
2. **Avisos de puertos locales.** Por omisión solo avisan los puertos nuevos accesibles desde otros equipos; los de 127.0.0.1 (muy frecuentes con herramientas de desarrollo) se ignoran. Se puede cambiar en la pantalla Avisos. ¿Te parece bien ese valor por omisión?
