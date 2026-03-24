import { BrowserRouter, Routes, Route } from 'react-router';
import { Toaster } from 'sonner';
import { TooltipProvider } from '@/components/ui/tooltip';
import { ConnectionProvider } from '@/hooks/use-connection';
import { SSEProvider } from '@/hooks/use-sse';
import { AppLayout } from '@/components/layout/app-layout';
import { DashboardPage } from '@/pages/dashboard';
import { SettingsPage } from '@/pages/settings';
import { OceanPage } from '@/pages/ocean';
import { ConnectPage } from '@/pages/connect';
import { SessionDetailPage } from '@/pages/session-detail';
import { SchedulesPage } from '@/pages/schedules';
import { WorktreesPage } from '@/pages/worktrees';

export function App() {
  return (
    <ConnectionProvider>
      <SSEProvider>
        <TooltipProvider>
          <BrowserRouter>
            <Routes>
              <Route element={<AppLayout />}>
                <Route index element={<OceanPage />} />
                <Route path="sessions" element={<DashboardPage />} />
                <Route path="sessions/:id" element={<SessionDetailPage />} />
                <Route path="worktrees" element={<WorktreesPage />} />
                <Route path="schedules" element={<SchedulesPage />} />
                <Route path="settings" element={<SettingsPage />} />
              </Route>
              <Route path="connect" element={<ConnectPage />} />
            </Routes>
          </BrowserRouter>
          <Toaster theme="dark" richColors position="bottom-right" />
        </TooltipProvider>
      </SSEProvider>
    </ConnectionProvider>
  );
}
