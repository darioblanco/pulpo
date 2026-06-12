import { SlashCommandBuilder, } from 'discord.js';
import { sessionEmbed } from '../formatters/embed.js';
export const data = new SlashCommandBuilder()
    .setName('spawn')
    .setDescription('Spawn a new agent session')
    .addStringOption((opt) => opt.setName('repo').setDescription('Repository path on the pulpod host').setRequired(true))
    .addStringOption((opt) => opt.setName('prompt').setDescription('Task prompt for the agent').setRequired(true))
    .addStringOption((opt) => opt.setName('persona').setDescription('Persona name (from pulpod config)').setRequired(false))
    .addStringOption((opt) => opt.setName('model').setDescription('Model override (e.g. opus, sonnet)').setRequired(false))
    .addStringOption((opt) => opt
    .setName('name')
    .setDescription('Session name (auto-generated if omitted)')
    .setRequired(false));
export async function execute(interaction, client) {
    await interaction.deferReply();
    const repo = interaction.options.getString('repo', true);
    const prompt = interaction.options.getString('prompt', true);
    const persona = interaction.options.getString('persona') ?? undefined;
    const model = interaction.options.getString('model') ?? undefined;
    const name = interaction.options.getString('name') ?? undefined;
    try {
        const session = await client.createSession({
            repo_path: repo,
            prompt,
            persona,
            model,
            name,
            metadata: {
                discord_channel_id: interaction.channelId,
                discord_user_id: interaction.user.id,
            },
        });
        const embed = sessionEmbed(session);
        await interaction.editReply({ embeds: [embed] });
    }
    catch (err) {
        const msg = err instanceof Error ? err.message : String(err);
        await interaction.editReply(`Failed to spawn session: ${msg}`);
    }
}
//# sourceMappingURL=spawn.js.map