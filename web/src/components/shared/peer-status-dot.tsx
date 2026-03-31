import { Tooltip, TooltipContent, TooltipProvider, TooltipTrigger } from '@/components/ui/tooltip';

const statusDotColors: Record<string, string> = {
  online: 'bg-status-ready',
  offline: 'bg-status-stopped',
  unknown: 'bg-muted-foreground',
};

interface PeerStatusDotProps {
  name: string;
  address: string;
  status: string;
  testId?: string;
}

export function PeerStatusDot({ name, address, status, testId }: PeerStatusDotProps) {
  return (
    <TooltipProvider>
      <Tooltip>
        <TooltipTrigger asChild>
          <span
            data-testid={testId}
            className={`h-2 w-2 shrink-0 rounded-full ${statusDotColors[status] ?? 'bg-muted-foreground'}`}
          />
        </TooltipTrigger>
        <TooltipContent>
          {name} ({address}) — {status === 'offline' ? 'Offline — last probe failed' : status}
        </TooltipContent>
      </Tooltip>
    </TooltipProvider>
  );
}
