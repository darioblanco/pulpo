import { useEffect, useState } from 'react';
import {
  Dialog,
  DialogContent,
  DialogHeader,
  DialogTitle,
  DialogDescription,
} from '@/components/ui/dialog';
import { TerminalView } from '@/components/session/terminal-view';
import { resumeSession } from '@/api/client';

interface AttachModalProps {
  sessionName: string;
  sessionId: string;
  sessionStatus: string;
  open: boolean;
  onOpenChange: (open: boolean) => void;
}

const LIVE_STATUSES = ['active', 'idle', 'creating'];
const RESUMABLE_STATUSES = ['lost', 'ready'];

export function AttachModal({
  sessionName,
  sessionId,
  sessionStatus,
  open,
  onOpenChange,
}: AttachModalProps) {
  const [ready, setReady] = useState(LIVE_STATUSES.includes(sessionStatus));
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    if (!open || ready) return;
    if (!RESUMABLE_STATUSES.includes(sessionStatus)) return;

    let cancelled = false;
    resumeSession(sessionName)
      .then(() => {
        if (!cancelled) setReady(true);
      })
      .catch((e: unknown) => {
        if (!cancelled) setError(e instanceof Error ? e.message : String(e));
      });
    return () => {
      cancelled = true;
    };
  }, [open, ready, sessionName, sessionStatus]);

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent
        className="flex h-[85vh] max-h-[90vh] w-[95vw] max-w-[95vw] flex-col gap-0 p-0 sm:max-w-[90vw]"
        data-testid="attach-modal"
      >
        <DialogHeader className="border-b px-4 py-3">
          <DialogTitle className="font-mono text-sm">{sessionName}</DialogTitle>
          <DialogDescription className="sr-only">
            Session terminal for {sessionName}
          </DialogDescription>
        </DialogHeader>
        <div className="min-h-0 flex-1 overflow-hidden" data-testid="attach-modal-body">
          {error ? (
            <div className="flex h-full items-center justify-center text-sm text-destructive">
              Failed to resume: {error}
            </div>
          ) : ready ? (
            <TerminalView
              sessionId={sessionId}
              className="h-full w-full min-w-0 overflow-hidden bg-[#0a1628]"
            />
          ) : (
            <div className="flex h-full items-center justify-center text-sm text-muted-foreground">
              Resuming session…
            </div>
          )}
        </div>
      </DialogContent>
    </Dialog>
  );
}
