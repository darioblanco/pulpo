import { useState, useEffect, useRef, useCallback } from 'react';
import { Button } from '@/components/ui/button';
import { Input } from '@/components/ui/input';
import { ScrollArea } from '@/components/ui/scroll-area';
import { getSessionOutput, sendInput } from '@/api/client';

interface ChatMessage {
  type: 'user' | 'agent';
  text: string;
}

function stripAnsi(text: string): string {
  // eslint-disable-next-line no-control-regex
  return text.replace(/\x1B\[[0-9;]*[a-zA-Z]/g, '');
}

interface ChatViewProps {
  sessionId: string;
  sessionStatus: string;
}

export function ChatView({ sessionId, sessionStatus }: ChatViewProps) {
  const [messages, setMessages] = useState<ChatMessage[]>([]);
  const [inputText, setInputText] = useState('');
  const previousOutputLenRef = useRef(0);
  const messagesEndRef = useRef<HTMLDivElement>(null);

  const scrollToBottom = useCallback(() => {
    messagesEndRef.current?.scrollIntoView({ behavior: 'smooth' });
  }, []);

  const fetchAndUpdate = useCallback(async () => {
    try {
      const data = await getSessionOutput(sessionId);
      const output = stripAnsi(data.output || '');

      if (output.length > previousOutputLenRef.current) {
        const delta = output.slice(previousOutputLenRef.current);
        previousOutputLenRef.current = output.length;

        setMessages((prev) => {
          if (prev.length === 0 || prev[prev.length - 1].type === 'user') {
            return [...prev, { type: 'agent', text: delta }];
          }
          const last = prev[prev.length - 1];
          return [...prev.slice(0, -1), { ...last, text: last.text + delta }];
        });
        scrollToBottom();
      }
    } catch {
      // Silently ignore fetch errors
    }
  }, [sessionId, scrollToBottom]);

  useEffect(() => {
    fetchAndUpdate();
    if (sessionStatus === 'running') {
      const interval = setInterval(fetchAndUpdate, 2000);
      return () => clearInterval(interval);
    }
  }, [sessionStatus, fetchAndUpdate]);

  async function handleSend() {
    if (!inputText.trim()) return;
    const text = inputText;
    setMessages((prev) => [...prev, { type: 'user', text }]);
    setInputText('');
    await sendInput(sessionId, text + '\n');
    scrollToBottom();
  }

  return (
    <div data-testid="chat-view">
      <ScrollArea className="max-h-[400px]" data-testid="chat-messages">
        <div className="space-y-3 p-2">
          {messages.map((msg, i) => (
            <div
              key={i}
              className={`flex ${msg.type === 'user' ? 'justify-end' : 'justify-start'}`}
            >
              <div
                className={`max-w-[80%] rounded-lg px-3 py-2 text-sm whitespace-pre-wrap ${
                  msg.type === 'user' ? 'bg-primary text-primary-foreground' : 'bg-muted'
                }`}
              >
                <p className="mb-1 text-xs font-medium opacity-70">
                  {msg.type === 'user' ? 'You' : 'Agent'}
                </p>
                {msg.text}
              </div>
            </div>
          ))}
          <div ref={messagesEndRef} />
        </div>
      </ScrollArea>

      {sessionStatus === 'running' && (
        <div className="mt-2 flex gap-2">
          <Input
            data-testid="chat-input"
            placeholder="Type a message..."
            value={inputText}
            onChange={(e) => setInputText(e.target.value)}
            onKeyDown={(e) => {
              if (e.key === 'Enter') handleSend();
            }}
          />
          <Button onClick={handleSend} size="sm">
            Send
          </Button>
        </div>
      )}
    </div>
  );
}
