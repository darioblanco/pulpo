import { SlashCommandBuilder, } from 'discord.js';
export const data = new SlashCommandBuilder()
    .setName('kill')
    .setDescription('Kill a running session')
    .addStringOption((opt) => opt
    .setName('session')
    .setDescription('Session name or ID')
    .setRequired(true)
    .setAutocomplete(true));
export async function execute(interaction, client) {
    await interaction.deferReply();
    const sessionId = interaction.options.getString('session', true);
    try {
        await client.killSession(sessionId);
        await interaction.editReply(`Session \`${sessionId}\` killed.`);
    }
    catch (err) {
        const msg = err instanceof Error ? err.message : String(err);
        await interaction.editReply(`Failed to kill session: ${msg}`);
    }
}
//# sourceMappingURL=kill.js.map