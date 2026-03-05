import { useState, useEffect, useRef, useCallback } from 'react';
import { Button } from '@/components/ui/button';
import { Input } from '@/components/ui/input';
import { ScrollArea } from '@/components/ui/scroll-area';
import { getSessionOutput, sendInput } from '@/api/client';

function stripAnsi(text: string): string {
  // eslint-disable-next-line no-control-regex
  return text.replace(/\x1B\[[0-9;]*[a-zA-Z]/g, '');
}

interface OutputViewProps {
  sessionId: string;
  sessionStatus: string;
}

export function OutputView({ sessionId, sessionStatus }: OutputViewProps) {
  const [output, setOutput] = useState('');
  const [inputText, setInputText] = useState('');
  const scrollRef = useRef<HTMLDivElement>(null);

  const fetchOutput = useCallback(async () => {
    try {
      const data = await getSessionOutput(sessionId);
      setOutput(stripAnsi(data.output || ''));
    } catch {
      // Silently ignore fetch errors
    }
  }, [sessionId]);

  useEffect(() => {
    fetchOutput();
    if (sessionStatus === 'running' || sessionStatus === 'stale') {
      const interval = setInterval(fetchOutput, 2000);
      return () => clearInterval(interval);
    }
  }, [sessionStatus, fetchOutput]);

  useEffect(() => {
    const el = scrollRef.current;
    if (el) {
      el.scrollTop = el.scrollHeight;
    }
  }, [output]);

  async function handleSend() {
    if (!inputText.trim()) return;
    await sendInput(sessionId, inputText + '\n');
    setInputText('');
  }

  const canSendInput = sessionStatus === 'running' || sessionStatus === 'stale';

  return (
    <div data-testid="output-view">
      <div className="overflow-hidden rounded-lg border border-border bg-[#0a1628]">
        <ScrollArea className="max-h-[400px]" ref={scrollRef}>
          <pre className="break-all p-3 font-mono text-xs leading-relaxed text-[#e0e0e0] whitespace-pre-wrap">
            {output || <span className="text-muted-foreground italic">No output yet</span>}
          </pre>
        </ScrollArea>
      </div>

      {canSendInput && (
        <div className="mt-2 flex gap-2">
          <Input
            data-testid="output-input"
            placeholder="Send input to session..."
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
