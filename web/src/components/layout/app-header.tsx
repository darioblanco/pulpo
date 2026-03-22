import { SidebarTrigger } from '@/components/ui/sidebar';
import { Separator } from '@/components/ui/separator';

interface AppHeaderProps {
  title: string;
  children?: React.ReactNode;
}

export function AppHeader({ title, children }: AppHeaderProps) {
  return (
    <header
      className="flex h-12 items-center gap-3 border-b border-border px-4"
      data-testid="app-header"
    >
      {/* Sidebar trigger hidden on mobile — bottom tab bar handles navigation */}
      <span className="hidden md:inline-flex">
        <SidebarTrigger />
      </span>
      <Separator orientation="vertical" className="hidden h-5 md:block" />
      <h1 className="text-sm font-medium">{title}</h1>
      {children && <div className="ml-auto flex items-center gap-2">{children}</div>}
    </header>
  );
}
