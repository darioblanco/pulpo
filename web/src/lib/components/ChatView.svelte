<script lang="ts">
  import { onMount, onDestroy, tick } from 'svelte';
  import { Messages as KMessages, Message, Messagebar, Link } from 'konsta/svelte';
  import { getSessionOutput, sendInput } from '$lib/api';

  // Konsta Messages type definitions are missing the children prop
  const Messages = KMessages as typeof KMessages & {
    new (...args: unknown[]): { $$prop_def: { children?: unknown } };
  };

  interface ChatMessage {
    type: 'user' | 'agent';
    text: string;
  }

  let {
    sessionId,
    sessionStatus,
  }: {
    sessionId: string;
    sessionStatus: string;
  } = $props();

  let messages: ChatMessage[] = $state([]);
  let previousOutputLen = 0;
  let inputText = $state('');
  let messagesEl: HTMLDivElement | undefined = $state(undefined);
  let polling: ReturnType<typeof setInterval> | null = null;

  function stripAnsi(text: string): string {
    // eslint-disable-next-line no-control-regex
    return text.replace(/\x1B\[[0-9;]*[a-zA-Z]/g, '');
  }

  async function scrollToBottom() {
    await tick();
    if (messagesEl) {
      messagesEl.scrollTop = messagesEl.scrollHeight;
    }
  }

  async function fetchAndUpdate() {
    try {
      const data = await getSessionOutput(sessionId);
      const output = stripAnsi(data.output || '');

      if (output.length > previousOutputLen) {
        const delta = output.slice(previousOutputLen);

        if (messages.length === 0 || messages[messages.length - 1].type === 'user') {
          messages = [...messages, { type: 'agent', text: delta }];
        } else {
          const last = messages[messages.length - 1];
          messages = [...messages.slice(0, -1), { ...last, text: last.text + delta }];
        }
        previousOutputLen = output.length;
        scrollToBottom();
      }
    } catch {
      // Silently ignore fetch errors
    }
  }

  async function handleSend() {
    if (!inputText.trim()) return;
    const text = inputText;
    messages = [...messages, { type: 'user', text }];
    inputText = '';
    await sendInput(sessionId, text + '\n');
    scrollToBottom();
  }

  function handleInput(e: Event) {
    inputText = (e.target as HTMLTextAreaElement).value;
  }

  onMount(() => {
    fetchAndUpdate();
    if (sessionStatus === 'running') {
      polling = setInterval(fetchAndUpdate, 2000);
    }
  });

  onDestroy(() => {
    if (polling) clearInterval(polling);
  });
</script>

<div bind:this={messagesEl} class="overflow-y-auto max-h-[400px]" data-testid="chat-messages">
  <Messages>
    {#each messages as msg, i (i)}
      <Message
        type={msg.type === 'user' ? 'sent' : 'received'}
        name={msg.type === 'user' ? 'You' : 'Agent'}
        text={msg.text}
      />
    {/each}
  </Messages>
</div>

{#if sessionStatus === 'running'}
  <Messagebar placeholder="Type a message..." value={inputText} onInput={handleInput}>
    {#snippet right()}
      <Link onClick={handleSend}>Send</Link>
    {/snippet}
  </Messagebar>
{/if}
