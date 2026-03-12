import { LayoutDashboard, History, BookOpen, Waves, Settings } from 'lucide-react';
import { Link, useLocation } from 'react-router';
import {
  Sidebar,
  SidebarContent,
  SidebarGroup,
  SidebarGroupContent,
  SidebarGroupLabel,
  SidebarHeader,
  SidebarMenu,
  SidebarMenuButton,
  SidebarMenuItem,
} from '@/components/ui/sidebar';
import { useSSE } from '@/hooks/use-sse';

const navItems = [
  { to: '/', icon: LayoutDashboard, label: 'Dashboard' },
  { to: '/history', icon: History, label: 'History' },
  { to: '/culture', icon: BookOpen, label: 'Culture' },
  { to: '/ocean', icon: Waves, label: 'Ocean' },
  { to: '/settings', icon: Settings, label: 'Settings' },
];

export function AppSidebar() {
  const { connected } = useSSE();
  const location = useLocation();

  return (
    <Sidebar data-testid="app-sidebar">
      <SidebarHeader className="border-b border-border px-4 py-3">
        <div className="flex items-center gap-2">
          <span className="font-display text-lg font-semibold text-primary">pulpo</span>
          <span
            data-testid="connection-dot"
            className={`h-2 w-2 rounded-full ${connected ? 'bg-status-finished' : 'bg-status-killed'}`}
          />
        </div>
      </SidebarHeader>
      <SidebarContent>
        <SidebarGroup>
          <SidebarGroupLabel>Navigation</SidebarGroupLabel>
          <SidebarGroupContent>
            <SidebarMenu>
              {navItems.map((item) => {
                const isActive =
                  item.to === '/'
                    ? location.pathname === '/'
                    : location.pathname.startsWith(item.to);
                return (
                  <SidebarMenuItem key={item.to}>
                    <SidebarMenuButton asChild isActive={isActive}>
                      <Link to={item.to}>
                        <item.icon className="h-4 w-4" />
                        <span>{item.label}</span>
                      </Link>
                    </SidebarMenuButton>
                  </SidebarMenuItem>
                );
              })}
            </SidebarMenu>
          </SidebarGroupContent>
        </SidebarGroup>
      </SidebarContent>
    </Sidebar>
  );
}
