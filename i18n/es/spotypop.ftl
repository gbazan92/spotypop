## Común

play = Reproducir
cancel = Cancelar
connect = Conectar
back = Volver
refresh = Actualizar
copy = Copiar
copied = Copiado
playlist = Playlist
track-count = { $count ->
    [one] { $count } canción
   *[other] { $count } canciones
}

## Reproductor

status-playing-on = Reproduciendo en { $device }
status-paused-on = En pausa en { $device }
status-paused = En pausa
status-loading = Cargando…
nothing-playing = Nada sonando
devices = Dispositivos
devices-searching = Buscando dispositivos…
devices-local-starting = El reproductor de esta computadora está arrancando; probá de nuevo en unos segundos.
devices-none = No hay dispositivos. Activá la reproducción en esta computadora o abrí Spotify en otro lado.
hero-last = Último: { $track }
hero-pick-below = Elegí algo de abajo para escucharlo acá.
hero-start-elsewhere = Iniciá Spotify en algún dispositivo para verlo acá.
previous = Anterior
next = Siguiente
mute = Silenciar
repeat-off = Repetir: no
repeat-all = Repetir: todo
repeat-track = Repetir: canción
shuffle-on = Aleatorio: sí
shuffle-off = Aleatorio: no
library-save = Guardar en tu biblioteca
library-remove = Quitar de tu biblioteca

## Reproducción en esta computadora

playback-title = Escuchá en esta computadora
playback-lead = Aprobalo una vez en el navegador y la música sale por esta computadora, sin abrir Spotify.
playback-link = Vincular
playback-unlink = Desvincular
playback-missing = Falta el reproductor local; reinstalá el applet con «just install».
playback-linked-as = Vinculada como «{ $device }»
playback-not-linked = Sin vincular
playback-not-installed = No instalada
playback-ready = Listo, esta computadora ya puede reproducir
playback-binary-missing = Falta { $binary } junto al applet; reinstalalo con «just install».
waiting-browser = Esperando al navegador…
waiting-browser-approval = Esperando que apruebes en el navegador…
player-not-found = No se encontró { $binary }
player-start-failed = No se pudo iniciar el reproductor: { $error }
player-not-authorized = Spotify no autorizó esta computadora.
player-auth-failed = La autorización falló: { $reason }
player-auth-incomplete = La autorización no se completó.

## Biblioteca

tab-search = Buscar
tab-queue = Cola
tab-playlists = Playlists
tab-podcasts = Podcasts
search-placeholder = Buscar en Spotify
searching = Buscando…
no-results = Sin resultados.
recently-played = Escuchado recientemente
liked-songs = Tus me gusta
liked-detail = { $count ->
    [one] Playlist  ·  { $count } canción
   *[other] Playlist  ·  { $count } canciones
}
empty-list = Esta lista está vacía.
empty-recent = Todavía no escuchaste nada. Buscá algo para empezar.
empty-queue = La cola está vacía. Agregá un tema con +.
empty-playlists = Todavía no tenés playlists.
empty-podcasts = No seguís ningún podcast. Buscá uno y seguilo en Spotify.
playlist-forbidden = Por limitaciones y restricciones de la API de Spotify, no puedo mostrarte las canciones. No te preocupes, podés reproducirlas y verlas en la sección Cola.
group-tracks = Canciones
group-artists = Artistas
group-albums = Álbumes
group-playlists = Playlists
group-podcasts = Podcasts
group-episodes = Episodios
group-chapters = Capítulos
group-audiobooks = Audiolibros
kind-album = Álbum
kind-artist = Artista
kind-podcast = Podcast
kind-audiobook = Audiolibro
queue-add = Agregar a la cola
queue-added = En la cola
queue-remove = Sacar de la cola
queue-queued = En cola: { $track }
queue-removed = Saqué «{ $track }» de la cola

## Configuración inicial

session-expired = Sesión vencida
signed-out = Sin conectar
reauth-title = Tu sesión de Spotify venció
reauth-lead = Spotify limita cada inicio de sesión a seis meses. Conectate de nuevo y todo sigue como estaba.
setup-title = Conectá tu cuenta de Spotify
setup-lead = Pegá el Client ID de tu app de Spotify y tocá Conectar. Necesitás Spotify Premium.
howto-title = Cómo obtener tu Client ID
howto-step-app = Creá una app en el panel de desarrolladores de Spotify.
howto-step-redirect = En Redirect URIs agregá exactamente:
howto-step-web-api = Marcá Web API y guardá los cambios.
howto-step-paste = Copiá el Client ID de la app, pegalo arriba y tocá Conectar.

## Configuración

settings = Configuración
settings-account = Cuenta
settings-playback = Reproducción en esta computadora
settings-panel = Panel
settings-show-in-bar = Mostrar en la barra
settings-style = Estilo
look-cover = Portada y título
look-bars = Barras
look-wave = Ondas
look-fill = Relleno
your-account = tu cuenta
signed-in-as = Conectado como { $name }
sign-out = Cerrar sesión
reconnect = Reconectar

## Páginas del navegador al iniciar sesión

login-page-not-found = No encontrado.
login-page-ignored = Esta respuesta no corresponde a este inicio de sesión. Probá conectar de nuevo.
login-page-denied = Spotify informó: { $reason }. Podés cerrar esta pestaña.
login-page-malformed = La respuesta de Spotify llegó mal formada. Probá conectar de nuevo.
login-page-done = Conectado a Spotify. Podés cerrar esta pestaña y volver al panel.

## Errores

error-signed-out = No estás conectado a Spotify.
error-reauth = Tu sesión de Spotify venció. Conectate de nuevo.
error-no-client-id = Falta el Client ID de tu app de Spotify.
error-no-device = No hay ningún dispositivo de Spotify activo.
error-premium = Spotify Premium es necesario para controlar la reproducción.
error-rate-limited = Demasiadas solicitudes a Spotify. Reintentá en { $seconds }s.
error-forbidden = Spotify rechazó la solicitud: { $message }
error-not-found = No encontrado: { $message }
error-server = Error del servidor de Spotify: { $message }
error-http = Spotify respondió HTTP { $status }: { $message }
error-network = No se pudo contactar a Spotify: { $message }
error-too-large = Spotify envió una respuesta demasiado grande.
error-bad-argument = Dato inválido: { $message }
error-port-busy = El puerto { $port } está ocupado. Elegí otro puerto de redirección.
error-login-timeout = Se agotó el tiempo esperando la respuesta de Spotify.
error-login-denied = El inicio de sesión falló: { $reason }
error-browser = No se pudo abrir el navegador: { $message }
error-storage = No se pudo guardar la sesión: { $message }
