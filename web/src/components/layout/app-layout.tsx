import { Outlet } from 'react-router';
import { SidebarInset, SidebarProvider } from '@/components/ui/sidebar';
import { AppSidebar } from './app-sidebar';
import { DisconnectedBanner } from './disconnected-banner';
import { MobileNav } from './mobile-nav';

export function AppLayout() {
  return (
    <SidebarProvider>
      <AppSidebar />
      <SidebarInset>
        <DisconnectedBanner />
        <div className="pb-16 md:pb-0">
          <Outlet />
        </div>
      </SidebarInset>
      <MobileNav />
    </SidebarProvider>
  );
}
