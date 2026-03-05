import { useCallback } from 'react';
import { useNavigate, useSearchParams } from 'react-router';
import { ConnectForm } from '@/components/connect/connect-form';
import { SavedConnections } from '@/components/connect/saved-connections';
import { useConnection, type SavedConnection } from '@/hooks/use-connection';

export function ConnectPage() {
  const navigate = useNavigate();
  const [searchParams] = useSearchParams();
  const { setBaseUrl, setAuthToken, savedConnections, addSavedConnection, removeSavedConnection } =
    useConnection();

  const tokenFromUrl = searchParams.get('token') ?? '';

  const handleConnect = useCallback(
    (url: string, token: string, nodeName: string) => {
      setBaseUrl(url);
      setAuthToken(token);
      addSavedConnection({
        name: nodeName,
        url,
        token: token || undefined,
        lastConnected: new Date().toISOString(),
      });
      navigate('/');
    },
    [setBaseUrl, setAuthToken, addSavedConnection, navigate],
  );

  const handleSelectSaved = useCallback(
    (conn: SavedConnection) => {
      setBaseUrl(conn.url);
      setAuthToken(conn.token ?? '');
      addSavedConnection({ ...conn, lastConnected: new Date().toISOString() });
      navigate('/');
    },
    [setBaseUrl, setAuthToken, addSavedConnection, navigate],
  );

  return (
    <div data-testid="connect-page" className="flex min-h-screen items-center justify-center">
      <div className="w-full max-w-sm space-y-6 p-6">
        <h1 className="text-center font-display text-2xl font-bold">Connect to Pulpo</h1>

        <ConnectForm onConnect={handleConnect} initialToken={tokenFromUrl} />

        <SavedConnections
          connections={savedConnections}
          onSelect={handleSelectSaved}
          onRemove={removeSavedConnection}
        />
      </div>
    </div>
  );
}
