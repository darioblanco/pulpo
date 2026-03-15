import { EventSource } from 'eventsource';
import type { Client, TextChannel } from 'discord.js';
import type { PulpodClient, SessionEvent } from '../api/pulpod.js';
import { eventEmbed } from '../formatters/embed.js';
import type { BotConfig } from '../config.js';

export function startSseListener(
  discordClient: Client,
  pulpodClient: PulpodClient,
  config: BotConfig,
): EventSource {
  const url = pulpodClient.sseUrl();
  console.log(`[SSE] Connecting to ${url}`);

  const es = new EventSource(url);

  es.addEventListener('session', async (e: MessageEvent) => {
    try {
      const event: SessionEvent = JSON.parse(e.data);
      await handleSessionEvent(discordClient, pulpodClient, config, event);
    } catch (err) {
      console.error('[SSE] Failed to handle event:', err);
    }
  });

  es.addEventListener('open', () => {
    console.log('[SSE] Connected');
  });

  es.addEventListener('error', (err: Event) => {
    console.error('[SSE] Connection error (will auto-reconnect):', err);
  });

  return es;
}

async function handleSessionEvent(
  discordClient: Client,
  pulpodClient: PulpodClient,
  config: BotConfig,
  event: SessionEvent,
): Promise<void> {
  // Try to find the discord channel from session metadata
  let channelId = config.notificationChannelId;

  if (!channelId) {
    // Look up the session to find discord_channel_id in metadata
    try {
      const session = await pulpodClient.getSession(event.session_id);
      channelId = session.metadata?.discord_channel_id;
    } catch {
      // Session might be gone; fall back to notification channel
    }
  }

  if (!channelId) {
    console.log(`[SSE] No channel for event on session ${event.session_name}, skipping`);
    return;
  }

  try {
    const channel = await discordClient.channels.fetch(channelId);
    if (!channel?.isTextBased()) {
      console.warn(`[SSE] Channel ${channelId} is not a text channel`);
      return;
    }

    const embed = eventEmbed(event);
    await (channel as TextChannel).send({ embeds: [embed] });
  } catch (err) {
    console.error(`[SSE] Failed to send to channel ${channelId}:`, err);
  }
}
