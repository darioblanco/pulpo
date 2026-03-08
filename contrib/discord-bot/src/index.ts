import {
  Client,
  Collection,
  Events,
  GatewayIntentBits,
  REST,
  Routes,
  type AutocompleteInteraction,
  type ChatInputCommandInteraction,
  type RESTPostAPIChatInputApplicationCommandsJSONBody,
} from 'discord.js';
import { loadConfig } from './config.js';
import { PulpodClient } from './api/pulpod.js';
import { startSseListener } from './listeners/sse.js';
import * as spawn from './commands/spawn.js';
import * as status from './commands/status.js';
import * as logs from './commands/logs.js';
import * as kill from './commands/kill.js';
import * as resume from './commands/resume.js';
import * as inks from './commands/inks.js';
import * as input from './commands/input.js';

interface Command {
  data: { toJSON(): RESTPostAPIChatInputApplicationCommandsJSONBody; name: string };
  execute(interaction: ChatInputCommandInteraction, client: PulpodClient): Promise<void>;
}

const commands = new Collection<string, Command>();
commands.set('spawn', spawn);
commands.set('status', status);
commands.set('logs', logs);
commands.set('kill', kill);
commands.set('resume', resume);
commands.set('inks', inks);
commands.set('input', input);

async function handleAutocomplete(
  interaction: AutocompleteInteraction,
  client: PulpodClient,
): Promise<void> {
  const focused = interaction.options.getFocused();
  try {
    const sessions = await client.listSessions();
    const query = focused.toLowerCase();
    const choices = sessions
      .filter((s) => s.name.toLowerCase().includes(query) || s.id.toLowerCase().includes(query))
      .slice(0, 25)
      .map((s) => ({ name: `${s.name} (${s.status})`, value: s.name }));
    await interaction.respond(choices);
  } catch {
    await interaction.respond([]);
  }
}

async function main(): Promise<void> {
  const config = loadConfig();
  const pulpod = new PulpodClient(config);

  const client = new Client({
    intents: [GatewayIntentBits.Guilds],
  });

  // Register slash commands on ready
  client.once(Events.ClientReady, async (c) => {
    console.log(`Logged in as ${c.user.tag}`);

    // Register commands globally
    const rest = new REST().setToken(config.discordToken);
    const commandData = commands.map((cmd) => cmd.data.toJSON());

    try {
      await rest.put(Routes.applicationCommands(c.user.id), { body: commandData });
      console.log(`Registered ${commandData.length} slash commands`);
    } catch (err) {
      console.error('Failed to register commands:', err);
    }

    // Start SSE listener for push notifications
    startSseListener(client, pulpod, config);
  });

  // Handle interactions (autocomplete + slash commands)
  client.on(Events.InteractionCreate, async (interaction) => {
    if (interaction.isAutocomplete()) {
      await handleAutocomplete(interaction, pulpod);
      return;
    }

    if (!interaction.isChatInputCommand()) return;

    const command = commands.get(interaction.commandName);
    if (!command) return;

    try {
      await command.execute(interaction, pulpod);
    } catch (err) {
      console.error(`Command /${interaction.commandName} failed:`, err);
      const reply = interaction.deferred
        ? interaction.editReply('An unexpected error occurred.')
        : interaction.reply({ content: 'An unexpected error occurred.', ephemeral: true });
      await reply.catch(console.error);
    }
  });

  await client.login(config.discordToken);
}

main().catch((err) => {
  console.error('Fatal error:', err);
  process.exit(1);
});
