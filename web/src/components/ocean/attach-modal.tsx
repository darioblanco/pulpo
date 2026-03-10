import {
  Dialog,
  DialogContent,
  DialogHeader,
  DialogTitle,
  DialogDescription,
} from '@/components/ui/dialog';
import { OutputView } from '@/components/session/output-view';

interface AttachModalProps {
  sessionName: string;
  sessionId: string;
  sessionStatus: string;
  open: boolean;
  onOpenChange: (open: boolean) => void;
}

export function AttachModal({
  sessionName,
  sessionId,
  sessionStatus,
  open,
  onOpenChange,
}: AttachModalProps) {
  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent
        className="flex max-h-[90vh] w-[95vw] max-w-[95vw] flex-col gap-0 p-0 sm:max-w-[90vw]"
        data-testid="attach-modal"
      >
        <DialogHeader className="border-b px-4 py-3">
          <DialogTitle className="font-mono text-sm">{sessionName}</DialogTitle>
          <DialogDescription className="sr-only">
            Session terminal output for {sessionName}
          </DialogDescription>
        </DialogHeader>
        <div className="min-h-0 flex-1 overflow-auto" data-testid="attach-modal-body">
          <OutputView sessionId={sessionId} sessionStatus={sessionStatus} />
        </div>
      </DialogContent>
    </Dialog>
  );
}
