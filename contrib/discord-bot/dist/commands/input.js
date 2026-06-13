import { SlashCommandBuilder, } from 'discord.js';
export const data = new SlashCommandBuilder()
    .setName('input')
    .setDescription('Send text input to a running session')
    .addStringOption((opt) => opt
    .setName('session')
    .setDescription('Session name or ID')
    .setRequired(true)
    .setAutocomplete(true))
    .addStringOption((opt) => opt.setName('text').setDescription('Text to send to the session').setRequired(true));
export async function execute(interaction, client) {
    await interaction.deferReply();
    const sessionId = interaction.options.getString('session', true);
    const text = interaction.options.getString('text', true);
    try {
        await client.sendInput(sessionId, text);
        await interaction.editReply(`Sent input to \`${sessionId}\`.`);
    }
    catch (err) {
        const msg = err instanceof Error ? err.message : String(err);
        await interaction.editReply(`Failed to send input: ${msg}`);
    }
}
//# sourceMappingURL=input.js.map