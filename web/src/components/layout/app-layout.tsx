import { Outlet } from 'react-router';
import { SidebarInset, SidebarProvider } from '@/components/ui/sidebar';
import { AppSidebar } from './app-sidebar';
import { DisconnectedBanner } from './disconnected-banner';

export function AppLayout() {
  return (
    <SidebarProvider>
      <AppSidebar />
      <SidebarInset>
        <DisconnectedBanner />
        <Outlet />
      </SidebarInset>
    </SidebarProvider>
  );
}
