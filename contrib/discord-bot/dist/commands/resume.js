import { SlashCommandBuilder, } from 'discord.js';
import { sessionEmbed } from '../formatters/embed.js';
export const data = new SlashCommandBuilder()
    .setName('resume')
    .setDescription('Resume a stale session after reboot')
    .addStringOption((opt) => opt
    .setName('session')
    .setDescription('Session name or ID')
    .setRequired(true)
    .setAutocomplete(true));
export async function execute(interaction, client) {
    await interaction.deferReply();
    const sessionId = interaction.options.getString('session', true);
    try {
        const session = await client.resumeSession(sessionId);
        const embed = sessionEmbed(session);
        await interaction.editReply({ embeds: [embed] });
    }
    catch (err) {
        const msg = err instanceof Error ? err.message : String(err);
        await interaction.editReply(`Failed to resume session: ${msg}`);
    }
}
//# sourceMappingURL=resume.js.map