export function loadConfig() {
    const discordToken = process.env.DISCORD_TOKEN;
    if (!discordToken) {
        throw new Error('DISCORD_TOKEN environment variable is required');
    }
    const pulpodUrl = process.env.PULPOD_URL ?? 'http://localhost:7433';
    const pulpodToken = process.env.PULPOD_TOKEN ?? '';
    const notificationChannelId = process.env.DISCORD_NOTIFICATION_CHANNEL_ID || undefined;
    return { discordToken, pulpodUrl, pulpodToken, notificationChannelId };
}
//# sourceMappingURL=config.js.map