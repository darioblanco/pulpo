import { SlashCommandBuilder, } from 'discord.js';
import { personaListEmbed } from '../formatters/embed.js';
export const data = new SlashCommandBuilder()
    .setName('personas')
    .setDescription('List available persona configurations');
export async function execute(interaction, client) {
    await interaction.deferReply();
    try {
        const { personas } = await client.listPersonas();
        const embed = personaListEmbed(personas);
        await interaction.editReply({ embeds: [embed] });
    }
    catch (err) {
        const msg = err instanceof Error ? err.message : String(err);
        await interaction.editReply(`Failed to list personas: ${msg}`);
    }
}
//# sourceMappingURL=personas.js.map