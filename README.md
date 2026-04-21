# Resonance

A fast, feature-rich Discord music bot written in Rust. Plays YouTube audio in voice channels with full queue management, search with emoji selection, and optional Redis-backed state persistence.

Built with [Serenity](https://github.com/serenity-rs/serenity) and [Songbird](https://github.com/serenity-rs/songbird) (with DAVE/E2EE voice encryption support).

## Features

- **YouTube playback** via yt-dlp with optional PO-token mitigation for stricter YouTube anti-bot checks
- **YouTube search** with reaction-based selection (1/2/3 emoji picker)
- **Full queue system** -- skip, pause, resume, shuffle, remove, clear
- **Loop modes** -- off, track, or entire queue
- **Per-guild isolation** -- each server gets its own queue and settings
- **Optional normalization** -- normalized playback uses ffmpeg + `dynaudnorm`
- **Redis-backed state writes** -- queue, now-playing, settings, and history persist while the bot is running
- **Auto-disconnect** -- leaves voice after configurable idle timeout
- **Slash commands** -- modern Discord interaction, instant registration
- **No privileged intents** -- runs without Message Content or Server Members intents
- **Graceful shutdown** -- clean disconnect on SIGINT/SIGTERM

## Commands

| Command | Description |
|---------|-------------|
| `/play <query>` | Play a YouTube URL or search query |
| `/search <query>` | Search YouTube, pick from 3 results via reactions |
| `/queue` | Show the current queue |
| `/skip` | Skip the current track |
| `/pause` | Pause playback |
| `/resume` | Resume playback |
| `/stop` | Stop playback and clear the queue |
| `/nowplaying` | Show the currently playing track |
| `/shuffle` | Shuffle the queue |
| `/loop <mode>` | Set loop mode: `off`, `track`, or `queue` |
| `/remove <pos>` | Remove a track by queue position |
| `/clear` | Clear the queue (keeps current track) |
| `/join` | Join your voice channel |
| `/leave` | Leave the voice channel |

## Quick Start

### Prerequisites

- [Rust 1.91+](https://rustup.rs/)
- [yt-dlp](https://github.com/yt-dlp/yt-dlp)
- [libopus](https://opus-codec.org/) (or cmake to build from source)
- [Redis](https://redis.io/) (optional, for persistence)

### Setup

1. **Create a Discord bot** at the [Developer Portal](https://discord.com/developers/applications)
   - Create an application, go to Bot tab, copy the token

2. **Clone and configure**
   ```bash
   git clone https://github.com/jvz-devx/resonance.git
   cd resonance
   cp .env.example .env
   # Edit .env with your bot token
   ```

3. **Build and run**
   ```bash
   cargo build --release
   cargo run --release
   ```

4. **Invite the bot** -- the invite link is printed on startup:
   ```
   Invite link: https://discord.com/oauth2/authorize?client_id=YOUR_ID&permissions=3165184&scope=bot%20applications.commands
   ```

### NixOS / Nix

```bash
git clone https://github.com/jvz-devx/resonance.git
cd resonance
cp .env.example .env
# Edit .env with your bot token
nix develop
cargo run --release
```

The flake provides Rust 1.91, cmake, libopus, yt-dlp, ffmpeg, and Redis.

### Docker

```bash
cp .env.example .env
# Edit .env with your bot token
docker compose up -d
```

This starts the bot and a Redis instance. The bot auto-connects to Redis at `redis://redis:6379`.
It also starts a `pot-server` sidecar and injects `POT_SERVER_URL=http://pot-server:4416` into the bot container.

## Configuration

All configuration is via environment variables (`.env` file supported):

| Variable | Required | Default | Description |
|----------|----------|---------|-------------|
| `DISCORD_TOKEN` | Yes | -- | Discord bot token |
| `REDIS_URL` | No | `redis://127.0.0.1:6379` | Redis connection URL |
| `POT_SERVER_URL` | No | unset locally / `http://pot-server:4416` in Docker Compose | bgutil PO-token server URL for yt-dlp |
| `IDLE_TIMEOUT_SECS` | No | `300` | Auto-disconnect timeout (seconds) |
| `RUST_LOG` | No | `info` | Log level ([tracing](https://docs.rs/tracing-subscriber/latest/tracing_subscriber/filter/struct.EnvFilter.html) format) |

`POT_SERVER_URL` is optional but recommended for self-hosted Docker playback if YouTube starts returning bot-check challenges or 403/429 errors. The shipped Compose stack wires this to the bundled `pot-server` container automatically.

## Architecture

```
src/
  main.rs           -- Entry point, client setup, graceful shutdown
  bot.rs            -- Event handler (ready, interactions, reactions)
  commands/         -- 14 slash command handlers
  player/
    events.rs       -- Songbird track-end handler, auto-advance logic
  queue/
    mod.rs          -- VecDeque-based queue manager
    track.rs        -- TrackMetadata (serde-serializable)
  youtube/
    search.rs       -- rusty_ytdl search + URL resolution
  state/
    mod.rs          -- Per-guild state, TypeMap keys, safe accessors
    redis.rs        -- Redis persistence (queue, settings, history)
  utils/
    embeds.rs       -- Discord embed builders
    error.rs        -- BotError enum with user-friendly messages
```

**Key design decisions:**
- `DashMap<GuildId, Arc<Mutex<GuildState>>>` for lock-free guild lookup with per-guild locking
- Songbird's built-in `YoutubeDl` input for standard playback, with a custom ffmpeg normalization path when normalization is enabled
- `rusty_ytdl` for search only (pure Rust, no subprocess)
- Write-through Redis persistence (every queue/settings mutation persists immediately), but queue/now-playing are not restored automatically on startup
- Zero `unwrap()` / `expect()` calls -- all errors handled via `BotResult<T>`

## System Requirements

| Dependency | Purpose | Install |
|-----------|---------|---------|
| yt-dlp | YouTube audio download | `pip install yt-dlp` |
| libopus | Audio codec | `apt install libopus-dev` / `pacman -S opus` |
| cmake | Build opus from source (if no system lib) | `apt install cmake` |
| Redis | Queue persistence (optional) | `apt install redis-server` |

## License

[MIT](LICENSE)
