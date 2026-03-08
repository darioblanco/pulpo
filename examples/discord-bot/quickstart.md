# Discord Bot Quickstart

This uses the bot in `contrib/discord-bot/`.

## 1. Configure environment

```bash
cd contrib/discord-bot
cp ../../examples/discord-bot/.env.example .env
# edit .env with your values
```

If pulpod uses `bind = "public"` in `[node]`, set `PULPOD_TOKEN` in `.env`.

## 2. Install and start

```bash
npm install
npm run build
npm start
```

## 3. Use slash commands

- `/spawn workdir:/path/to/repo ink:reviewer prompt:Review auth flow`
- `/status`
- `/logs session:<name>`
- `/kill session:<name>`
- `/resume session:<name>`
- `/inks`

The bot also subscribes to `GET /api/v1/events` and posts updates back to Discord.

For full details, see [contrib/discord-bot/README.md](/Users/dario/Code/darioblanco/pulpo/contrib/discord-bot/README.md).
