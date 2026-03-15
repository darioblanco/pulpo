import { BrowserRouter, Routes, Route } from 'react-router';
import { Toaster } from 'sonner';
import { TooltipProvider } from '@/components/ui/tooltip';
import { ConnectionProvider } from '@/hooks/use-connection';
import { SSEProvider } from '@/hooks/use-sse';
import { AppLayout } from '@/components/layout/app-layout';
import { DashboardPage } from '@/pages/dashboard';
import { HistoryPage } from '@/pages/history';
import { SettingsPage } from '@/pages/settings';
import { OceanPage } from '@/pages/ocean';
import { ConnectPage } from '@/pages/connect';

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
                <Route path="history" element={<HistoryPage />} />
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
