import { Outlet } from 'react-router';
import { SidebarInset, SidebarProvider } from '@/components/ui/sidebar';
import { AppSidebar } from './app-sidebar';
import { DisconnectedBanner } from './disconnected-banner';
import { MobileNav } from './mobile-nav';

export function AppLayout() {
  return (
    <div className="flex h-[100dvh] flex-col md:h-auto md:min-h-svh md:flex-row">
      <SidebarProvider className="min-h-0 flex-1 md:min-h-svh">
        <AppSidebar />
        <SidebarInset className="min-w-0">
          <DisconnectedBanner />
          <div className="min-h-0 min-w-0 flex-1 overflow-y-auto overflow-x-hidden">
            <Outlet />
          </div>
        </SidebarInset>
      </SidebarProvider>
      <MobileNav />
    </div>
  );
}
