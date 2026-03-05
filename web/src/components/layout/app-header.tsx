import { SidebarTrigger } from '@/components/ui/sidebar';
import { Separator } from '@/components/ui/separator';

interface AppHeaderProps {
  title: string;
}

export function AppHeader({ title }: AppHeaderProps) {
  return (
    <header
      className="flex h-12 items-center gap-3 border-b border-border px-4"
      data-testid="app-header"
    >
      <SidebarTrigger />
      <Separator orientation="vertical" className="h-5" />
      <h1 className="text-sm font-medium">{title}</h1>
    </header>
  );
}
