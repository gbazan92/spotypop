## Common

play = Play
cancel = Cancel
connect = Connect
back = Back
refresh = Refresh
copy = Copy
copied = Copied
playlist = Playlist
track-count = { $count ->
    [one] { $count } song
   *[other] { $count } songs
}

## Player

status-playing-on = Playing on { $device }
status-paused-on = Paused on { $device }
status-paused = Paused
status-loading = Loading…
nothing-playing = Nothing playing
devices = Devices
devices-searching = Looking for devices…
devices-local-starting = The player on this computer is starting up; try again in a few seconds.
devices-none = No devices. Turn on playback on this computer or open Spotify somewhere else.
hero-last = Last played: { $track }
hero-pick-below = Pick something below to play it here.
hero-start-elsewhere = Start Spotify on any device to see it here.
previous = Previous
next = Next
mute = Mute
repeat-off = Repeat: off
repeat-all = Repeat: all
repeat-track = Repeat: song
shuffle-on = Shuffle: on
shuffle-off = Shuffle: off
library-save = Save to your library
library-remove = Remove from your library

## Playback on this computer

playback-title = Listen on this computer
playback-lead = Approve it once in the browser and the music plays from this computer, without opening Spotify.
playback-link = Link
playback-unlink = Unlink
playback-missing = The local player is missing; reinstall the applet with “just install”.
playback-linked-as = Linked as “{ $device }”
playback-not-linked = Not linked
playback-not-installed = Not installed
playback-ready = Done, this computer can play now
playback-binary-missing = { $binary } is missing next to the applet; reinstall it with “just install”.
waiting-browser = Waiting for the browser…
waiting-browser-approval = Waiting for you to approve in the browser…
player-not-found = { $binary } was not found
player-start-failed = Could not start the player: { $error }
player-not-authorized = Spotify did not authorize this computer.
player-auth-failed = Authorization failed: { $reason }
player-auth-incomplete = Authorization did not finish.

## Library

tab-search = Search
tab-queue = Queue
tab-playlists = Playlists
tab-podcasts = Podcasts
search-placeholder = Search Spotify
searching = Searching…
no-results = No results.
recently-played = Recently played
liked-songs = Liked Songs
liked-detail = { $count ->
    [one] Playlist  ·  { $count } song
   *[other] Playlist  ·  { $count } songs
}
empty-list = This list is empty.
empty-recent = You have not played anything yet. Search for something to start.
empty-queue = The queue is empty. Add a song with +.
empty-playlists = You have no playlists yet.
empty-podcasts = You do not follow any podcasts. Find one and follow it on Spotify.
playlist-forbidden = Because of Spotify API limits and restrictions, I cannot show you these songs. Don’t worry: you can still play them and see them in the Queue tab.
group-tracks = Songs
group-artists = Artists
group-albums = Albums
group-playlists = Playlists
group-podcasts = Podcasts
group-episodes = Episodes
group-chapters = Chapters
group-audiobooks = Audiobooks
kind-album = Album
kind-artist = Artist
kind-podcast = Podcast
kind-audiobook = Audiobook
queue-add = Add to queue
queue-added = In the queue
queue-remove = Remove from queue
queue-queued = Queued: { $track }
queue-removed = Removed “{ $track }” from the queue

## Setup

session-expired = Session expired
signed-out = Not connected
reauth-title = Your Spotify session expired
reauth-lead = Spotify limits each sign-in to six months. Connect again and everything picks up where it was.
setup-title = Connect your Spotify account
setup-lead = Paste the Client ID of your Spotify app and press Connect. You need Spotify Premium.
howto-title = How to get your Client ID
howto-step-app = Create an app in the Spotify developer dashboard.
howto-step-redirect = Under Redirect URIs, add exactly:
howto-step-web-api = Check Web API and save the changes.
howto-step-paste = Copy the app’s Client ID, paste it above and press Connect.

## Settings

settings = Settings
settings-account = Account
settings-playback = Playback on this computer
settings-panel = Panel
settings-show-in-bar = Show in the panel
settings-style = Style
look-cover = Cover and title
look-bars = Bars
look-wave = Wave
look-fill = Fill
your-account = your account
signed-in-as = Signed in as { $name }
sign-out = Sign out
reconnect = Reconnect

## Browser pages after signing in

login-page-not-found = Not found.
login-page-ignored = This reply does not belong to this sign-in. Try connecting again.
login-page-denied = Spotify said: { $reason }. You can close this tab.
login-page-malformed = Spotify’s reply arrived malformed. Try connecting again.
login-page-done = Connected to Spotify. You can close this tab and go back to the panel.

## Errors

error-signed-out = You are not connected to Spotify.
error-reauth = Your Spotify session expired. Connect again.
error-no-client-id = The Client ID of your Spotify app is missing.
error-no-device = No Spotify device is active.
error-premium = Spotify Premium is required to control playback.
error-rate-limited = Too many requests to Spotify. Try again in { $seconds }s.
error-forbidden = Spotify refused the request: { $message }
error-not-found = Not found: { $message }
error-server = Spotify server error: { $message }
error-http = Spotify answered HTTP { $status }: { $message }
error-network = Could not reach Spotify: { $message }
error-too-large = Spotify sent a reply that is too large.
error-bad-argument = Invalid value: { $message }
error-port-busy = Port { $port } is in use. Pick another redirect port.
error-login-timeout = Timed out waiting for Spotify’s reply.
error-login-denied = Sign-in failed: { $reason }
error-browser = Could not open the browser: { $message }
error-storage = Could not save the session: { $message }
