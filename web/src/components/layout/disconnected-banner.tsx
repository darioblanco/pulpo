import { useSSE } from '@/hooks/use-sse';

export function DisconnectedBanner() {
  const { connected } = useSSE();

  if (connected) return null;

  return (
    <div
      data-testid="disconnected-banner"
      className="fixed inset-x-0 top-0 z-50 flex items-center justify-center gap-2 bg-destructive px-4 py-2 text-sm font-medium text-white"
    >
      <span className="inline-block h-2 w-2 animate-pulse rounded-full bg-white" />
      Disconnected from pulpod — reconnecting...
    </div>
  );
}
